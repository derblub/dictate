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

//! Sitzungsautomat des Panels der Textaktionen (Port der Logik des früheren Awesome-Panels `actions.lua`, PLAN-wayland.md 6d
//! Punkt 2): rein, ohne GTK und ohne Uhr. Jede Eingabe trägt `now`, jede Folge ist ein [`Effect`], den der Hauptthread
//! von `dictate-ui` ausführt (Fenster zeigen, `act_*` an den Daemon, Copy, Replace). So prüfen die Unit-Tests auch
//! späte Antworten, Totzeit und Reihenfolge.
//!
//! Zustände `closed → grabbing → select → working → result | error` wie in Lua. `gen` steigt bei jedem Öffnen und
//! Schließen, `run_gen` bei jedem Start und Abbruch einer Aktion: eine Antwort mit älterer Nummer wird verworfen
//! (Befund 10). Die `act`-Aufrufe außer `run` laufen im Binary seriell (ein `clear` trifft nie eine neuere Markierung).
//!
//! Totzeit (Befund 9, gemeinsam mit der Diagnose in [`crate::ui::gate`]): nach jedem Zustandswechsel lösen für
//! [`crate::ui::gate::DEADTIME`] weder Tasten noch Klicks aus, und Tasten, die beim Wechsel gedrückt waren, erst nach
//! ihrem Loslassen (kein doppeltes `r`). Ein Drücken binnen [`crate::ui::gate::REPEAT_GAP`] nach dem Loslassen derselben
//! Taste gilt als Wiederholung (X schickte bei Autorepeat Loslassen und Drücken; GTK unter Wayland wiederholt nur mit
//! Drücken; beide Formen sind getestet).
//!
//! Unter niri hat das Panel als schwebendes Fenster den Fokus (anders als unter Awesome). Daraus folgt: Replace prüft
//! nicht „ist das Zielfenster fokussiert“, sondern ob es beim Öffnen bekannt war; der Ausgabe-Thread wartet nach dem
//! Schließen, bis niri den Fokus dorthin zurückgegeben hat, und schreibt nur dann (Befund 6). Fokusverlust des Panels
//! schließt es (Ersatz für „Klick daneben“).
//!
//! Text der Markierung und der Antwort lebt nur hier im Speicher und wird beim Schließen überschrieben; nie loggen.
//!
//! **Quellen** (W5 Block D, Port von `api.open_with` in `actions.lua`): Mod+I öffnet mit der Markierung
//! ([`Source::Selection`], [`Panel::toggle`]); die Befehlspalette öffnet dasselbe Panel über [`Panel::open_with`] mit
//! - [`Source::Question`] (Absicht `ask`): die Frage kommt aus der Palette (Feld [`Panel::question`], nie geloggt), die
//!   eine Aktion startet sofort nach `act_grab`; ohne Auswahl, zu der es zurückginge (Back und Esc schließen);
//! - [`Source::Clipboard`] (`explain_clipboard`): das Binary liest CLIPBOARD (nur auf diese Absicht), das Panel zeigt
//!   die Vorschau und sendet erst auf Return, Taste oder Klick auf die eine Aktion; Esc sendet nichts;
//! - [`Source::Selection`] mit `preselect` (`text_action`): Mod+I-Ablauf auf PRIMARY mit Vorschau, die vorgewählte
//!   Aktion ist hervorgehoben und startet erst auf Return, Taste oder Klick (Entscheidung 18: PRIMARY kann eine alte
//!   Markierung aus einem anderen Fenster sein, gesendet wird nie ungesehen); ein Geheimnis lässt den Sperrblock stehen.
//!
//! Aus der Palette geöffnet, meldet [`Effect::Chosen`] einmal je Öffnung das erste Ergebnis (Häufigkeit `pal_chosen`,
//! wie `on_success` in Lua; Lesart L34).

use crate::actions::store::wipe;
pub use crate::actions::store::Source;
use crate::ui::gate::{Gate, IDLE};
use crate::ui::markdown::DiffPiece;
use std::time::{Duration, Instant};

/// Der Wartezustand beim Lesen der Markierung erscheint erst nach dieser Zeit (kurze Grabs ohne Flackern)
pub const SHOW_DELAY: Duration = Duration::from_millis(150);

/// Meldung des Daemons, wenn er fehlt (gleicher Text wie im CLI)
pub const DAEMON_DOWN: &str = "The dictate daemon is not running";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Closed,
    Grabbing,
    Select,
    Working,
    Result,
    Error,
}

/// Eine Aktion aus der Antwort auf `act_grab`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionInfo {
    pub key: String,
    pub name: String,
    pub label: String,
    /// `answer` oder `replace`
    pub output: String,
    pub allowed: bool,
    pub model: String,
}

impl ActionInfo {
    pub fn replaces(&self) -> bool {
        self.output == "replace"
    }
}

/// Antwort auf `act_grab` (ohne den Text; die Vorschau berechnet das Binary vor dem Senden)
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GrabInfo {
    pub id: String,
    pub chars: u64,
    pub lines: u64,
    pub cloud: bool,
    pub host: String,
    pub model: String,
    /// Arten erkannter Geheimnisse (`secrets::KINDS`)
    pub secret: Vec<String>,
    pub replace_ok: bool,
    pub preview: String,
    pub actions: Vec<ActionInfo>,
}

impl RunEv {
    /// Endantwort von `act_run` (die Zeile mit `ok`), eine Lesart für Client und Tests: `ok` = Ergebnis mit Text,
    /// Ausgabeart, `truncated`, `note` und Wortdiff; sonst `cancelled` oder Fehlertext. Text und Diff-Stücke werden aus
    /// `v` herausgenommen (nicht kopiert); der Aufrufer überschreibt den Rest.
    pub fn from_reply(v: &mut serde_json::Value) -> RunEv {
        let take = |v: &mut serde_json::Value| match v.take() {
            serde_json::Value::String(s) => s,
            _ => String::new(),
        };
        if v["ok"] != true {
            if v["cancelled"] == true {
                return RunEv::Cancelled;
            }
            return RunEv::Error(v["error"].as_str().unwrap_or("command failed").to_string());
        }
        let diff = v["diff"]
            .as_array_mut()
            .map(|a| {
                a.iter_mut()
                    .filter_map(|p| {
                        let op = p[0].as_str().and_then(|o| o.chars().next())?;
                        Some(DiffPiece { op, text: take(&mut p[1]) })
                    })
                    .collect()
            })
            .unwrap_or_default();
        RunEv::Result {
            output: v["output"].as_str().unwrap_or("answer").to_string(),
            truncated: v["truncated"] == true,
            note: v["note"].as_str().map(str::to_string),
            text: take(&mut v["text"]),
            diff,
        }
    }
}

impl GrabInfo {
    /// Aus der JSON-Antwort des Daemons (`Request::ActGrab`); `None`, wenn Pflichtfelder fehlen
    pub fn from_json(v: &serde_json::Value, preview: String) -> Option<GrabInfo> {
        let s = |v: &serde_json::Value| v.as_str().unwrap_or("").to_string();
        let id = v["id"].as_str().filter(|i| !i.is_empty())?.to_string();
        let actions = v["actions"]
            .as_array()?
            .iter()
            .map(|a| ActionInfo {
                key: s(&a["key"]),
                name: s(&a["name"]),
                label: s(&a["label"]),
                output: s(&a["output"]),
                allowed: a["allowed"].as_bool().unwrap_or(false),
                model: s(&a["model"]),
            })
            .filter(|a| !a.name.is_empty())
            .collect();
        Some(GrabInfo {
            id,
            chars: v["chars"].as_u64().unwrap_or(0),
            lines: v["lines"].as_u64().unwrap_or(0),
            cloud: v["target"]["cloud"].as_bool().unwrap_or(false),
            host: s(&v["target"]["host"]),
            model: s(&v["target"]["model"]),
            secret: v["secret"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default(),
            replace_ok: v["replace_ok"].as_bool().unwrap_or(false),
            preview,
            actions,
        })
    }

    pub fn any_allowed(&self) -> bool {
        self.actions.iter().any(|a| a.allowed)
    }
}

/// Fenster, das beim Öffnen fokussiert war (niri-ID und `app_id`); `window: None` = unbekannt (dann kein Replace)
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Target {
    pub window: Option<u64>,
    pub app_id: Option<String>,
}

/// Fehler mit Anleitung (`grab_error`/`run_error` in Lua)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErrInfo {
    pub title: String,
    pub body: Option<String>,
    /// Kein Fehler, sondern ein Leerzustand (neutral gezeichnet)
    pub neutral: bool,
    /// „Try again“ (Aktion erneut)
    pub retry: bool,
    /// „Back“ zur Auswahl
    pub back: bool,
}

/// Ereignisse aus dem gestreamten `act_run`
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunEv {
    Delta(String),
    /// `note`: Hinweis einer wachsenden Aktion (länger als ihr Ziel)
    Result {
        output: String,
        truncated: bool,
        note: Option<String>,
        text: String,
        diff: Vec<DiffPiece>,
    },
    Cancelled,
    Error(String),
    /// Verbindung endete ohne Endantwort; `daemon_down`: kein Daemon erreichbar
    Ended {
        daemon_down: bool,
    },
}

/// Taste nach `gdk::Key::name()` (X-Keysym-Namen) samt Modifikatoren
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Mods {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub logo: bool,
}

impl Mods {
    /// Nur Shift oder nichts: sonst ist es kein Aktionskürzel (Super ist beim Öffnen oft noch gehalten)
    pub fn plain(self) -> bool {
        !self.ctrl && !self.alt && !self.logo
    }
}

/// Bildlauf in der Antwort (führt das GTK-Panel aus)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scroll {
    LineUp,
    LineDown,
    PageUp,
    PageDown,
    Top,
    Bottom,
}

/// Folgen für den Hauptthread
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// Text der Quelle lesen und `act_grab` mit `app` schicken (übersprungen, wenn `gen` bis zum Start veraltet ist):
    /// PRIMARY, CLIPBOARD bzw. die Frage aus [`Panel::question`]
    Grab {
        gen: u64,
        source: Source,
    },
    /// `act_run` starten (gestreamt, eigene Verbindung)
    Run {
        gen: u64,
        run_gen: u64,
        id: String,
        action: String,
    },
    /// Laufenden Aufruf im Daemon abbrechen (und den eigenen Strom beenden)
    CancelRun {
        id: String,
    },
    /// `act_clear`: Markierung und Ergebnis im Daemon löschen
    Clear,
    /// `act_result` holen und in CLIPBOARD legen (`block`: nur dieser Codeblock, ab 1)
    Copy {
        gen: u64,
        id: String,
        block: Option<usize>,
    },
    /// Nach dem Schließen den Text im beim Öffnen gemerkten Fenster ersetzen (Ausgabeweg aus W2)
    Replace {
        text: String,
        window: u64,
    },
    /// Fenster zeigen bzw. verbergen
    Show,
    Hide,
    /// Inhalt neu aufbauen
    Render,
    /// Nur die Hervorhebung (und den Hinweis dazu) neu setzen: Hover baut nichts neu auf, sonst träfe das Loslassen
    /// eines Klicks ein neues Element, das das Drücken nie sah
    Highlight,
    /// Nur den Text der Antwort neu setzen (Streaming; wer am Ende stand, bleibt am Ende)
    Stream,
    Scroll(Scroll),
    /// Hinweis im Overlay (nur Zahlen, nie Text)
    Notice(String),
    /// Einstellungsfenster öffnen (`page`)
    Settings(&'static str),
    /// Aus der Palette geöffnet: das erste Ergebnis ist da (`pal_chosen`, einmal je Öffnung; Ziel kennt das Binary)
    Chosen,
}

/// Art eines bedienbaren Elements
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemKind {
    /// Zeile der Aktionsliste
    Row,
    Primary,
    Secondary,
    Ghost,
}

/// Ein bedienbares Element des aktuellen Zustands (Knopf oder Zeile), in Reihenfolge für Tab
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub id: String,
    pub label: String,
    /// Tasten-Chip (`Enter`, `Esc`, `c`, …)
    pub key: Option<String>,
    pub kind: ItemKind,
    pub enabled: bool,
    /// Links in der Knopfzeile (Back, Show changes)
    pub left: bool,
}

fn item(id: &str, label: &str, key: Option<&str>, kind: ItemKind, left: bool) -> Item {
    Item { id: id.into(), label: label.into(), key: key.map(str::to_string), kind, enabled: true, left }
}

/// Vorschau der Markierung: erste [`PREVIEW_LINES`] Zeilen, höchstens [`PREVIEW_CHARS`] Zeichen (Rand ohne Leerraum, ohne
/// Steuerzeichen außer Zeilenumbruch und Tab); wie früher `client::preview` des CLI unter AwesomeWM
pub fn preview(text: &str) -> crate::actions::store::SecretText {
    let mut out = String::with_capacity(PREVIEW_CHARS * 4);
    let (mut lines, mut chars) = (1, 0);
    for c in text.trim().chars() {
        if chars == PREVIEW_CHARS {
            break;
        }
        if c == '\n' {
            if lines == PREVIEW_LINES {
                break;
            }
            lines += 1;
        } else if c.is_control() && c != '\t' {
            continue;
        }
        out.push(c);
        chars += 1;
    }
    crate::actions::store::SecretText::new(out)
}

pub const PREVIEW_LINES: usize = 6;
pub const PREVIEW_CHARS: usize = 400;

pub fn plural(n: u64, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

#[derive(Debug)]
pub struct Panel {
    pub phase: Phase,
    gen: u64,
    run_gen: u64,
    pub target: Target,
    pub grab: Option<GrabInfo>,
    pub action: Option<ActionInfo>,
    /// Antworttext (gestreamt bzw. fertig); nur im Speicher, beim Schließen überschrieben
    pub text: String,
    pub note: Option<String>,
    pub truncated: bool,
    pub result_output: Option<String>,
    pub diff: Vec<DiffPiece>,
    pub show_marks: bool,
    pub err: Option<ErrInfo>,
    /// Fehlerzeile im Panel (Copy gescheitert)
    pub notice: Option<String>,
    /// Hervorgehobenes Element (Hover und ↑/↓/Tab teilen sie)
    pub active: Option<String>,
    /// Fenster gezeigt
    pub visible: bool,
    /// Start der laufenden Aktion (Sekunden im Kopf)
    pub run_started: Option<Instant>,
    /// Copy läuft (kein zweites)
    copying: bool,
    /// Ein `act_run` läuft (beim Abbruch `act_cancel`)
    running: bool,
    opened_at: Option<Instant>,
    /// Totzeit, alte Tasten, Leerlauf
    pub gate: Gate,
    /// Herkunft des Texts dieser Öffnung
    pub source: Source,
    /// Frage aus der Palette (nur [`Source::Question`]); nur im Speicher, beim Schließen überschrieben
    pub question: String,
    /// Vorgewählte Aktion (`text_action` der Palette)
    preselect: Option<String>,
    /// Aus der Palette geöffnet und noch kein Ergebnis: [`Effect::Chosen`] steht aus
    chosen_pending: bool,
}

impl Default for Panel {
    fn default() -> Self {
        Panel::new()
    }
}

impl Panel {
    pub fn new() -> Panel {
        Panel {
            phase: Phase::Closed,
            gen: 0,
            run_gen: 0,
            target: Target::default(),
            grab: None,
            action: None,
            text: String::new(),
            note: None,
            truncated: false,
            result_output: None,
            diff: Vec::new(),
            show_marks: true,
            err: None,
            notice: None,
            active: None,
            visible: false,
            run_started: None,
            copying: false,
            running: false,
            opened_at: None,
            gate: Gate::new(),
            source: Source::Selection,
            question: String::new(),
            preselect: None,
            chosen_pending: false,
        }
    }

    pub fn gen(&self) -> u64 {
        self.gen
    }

    pub fn run_gen(&self) -> u64 {
        self.run_gen
    }

    pub fn is_open(&self) -> bool {
        self.phase != Phase::Closed
    }

    /// Mod+I: offen schließt, zu öffnet mit der Markierung (der Fokus wurde vorher gemerkt)
    pub fn toggle(&mut self, target: Target, now: Instant) -> Vec<Effect> {
        if self.is_open() {
            return self.close();
        }
        self.start(Source::Selection, target, now)
    }

    /// Aus der Befehlspalette öffnen (Modulkopf): `question` nur für [`Source::Question`], `preselect` nur für
    /// [`Source::Selection`]. Ein offenes Panel schließt vorher (wie in Lua)
    pub fn open_with(&mut self, source: Source, preselect: Option<&str>, question: Option<String>, target: Target, now: Instant) -> Vec<Effect> {
        let mut out = self.close();
        self.question = if source == Source::Question { question.unwrap_or_default() } else { String::new() };
        self.preselect = if source == Source::Selection { preselect.map(str::to_string) } else { None };
        self.chosen_pending = true;
        out.extend(self.start(source, target, now));
        out
    }

    fn start(&mut self, source: Source, target: Target, now: Instant) -> Vec<Effect> {
        self.gen += 1;
        self.source = source;
        self.target = target;
        self.phase = Phase::Grabbing;
        self.opened_at = Some(now);
        self.gate.open(now);
        vec![Effect::Grab { gen: self.gen, source }]
    }

    /// Schließen (Esc, ✕, Fokusverlust, zweites Mod+I, Sperre, Leerlauf): laufenden Aufruf abbrechen, Sitzung im
    /// Daemon löschen, Text überschreiben
    pub fn close(&mut self) -> Vec<Effect> {
        if !self.is_open() {
            return vec![];
        }
        self.gen += 1;
        self.run_gen += 1;
        let mut out = self.stop_run_effects();
        out.push(Effect::Clear);
        self.phase = Phase::Closed;
        self.wipe_result();
        if let Some(g) = &mut self.grab {
            wipe(&mut g.preview);
        }
        self.grab = None;
        self.action = None;
        self.err = None;
        self.target = Target::default();
        self.active = None;
        self.copying = false;
        self.opened_at = None;
        self.gate.clear();
        wipe(&mut self.question);
        self.question = String::new();
        self.preselect = None;
        self.chosen_pending = false;
        self.source = Source::Selection;
        if self.visible {
            self.visible = false;
            out.push(Effect::Hide);
        }
        out
    }

    fn wipe_result(&mut self) {
        wipe(&mut self.text);
        self.text = String::new();
        for d in &mut self.diff {
            wipe(&mut d.text);
        }
        self.diff.clear();
        self.note = None;
        self.truncated = false;
        self.result_output = None;
        self.notice = None;
        self.run_started = None;
    }

    fn stop_run_effects(&mut self) -> Vec<Effect> {
        if !std::mem::take(&mut self.running) {
            return vec![];
        }
        match &self.grab {
            Some(g) => vec![Effect::CancelRun { id: g.id.clone() }],
            None => vec![],
        }
    }

    /// Zustandswechsel mit Totzeit (Grundsatz 3 in UX-actions.md)
    fn set_phase(&mut self, p: Phase, now: Instant, out: &mut Vec<Effect>) {
        if self.phase != p {
            self.phase = p;
            self.gate.changed(now);
            self.active = self.default_active();
        }
        if !self.visible {
            self.visible = true;
            out.push(Effect::Show);
        }
        out.push(Effect::Render);
    }

    /// Nächster Zeitpunkt, zu dem [`Panel::tick`] etwas tun kann (Wartezustand zeigen, Leerlauf)
    pub fn next_deadline(&self) -> Option<Instant> {
        match self.phase {
            Phase::Closed => None,
            Phase::Grabbing if !self.visible => self.opened_at.map(|t| t + SHOW_DELAY),
            Phase::Working => None,
            _ => self.gate.last_input().map(|t| t + IDLE),
        }
    }

    pub fn tick(&mut self, now: Instant) -> Vec<Effect> {
        match self.phase {
            Phase::Grabbing if !self.visible && self.opened_at.is_some_and(|t| now >= t + SHOW_DELAY) => {
                self.visible = true;
                vec![Effect::Show, Effect::Render]
            }
            Phase::Closed | Phase::Working | Phase::Grabbing => vec![],
            _ if self.gate.idle(now) => self.close(),
            _ => vec![],
        }
    }

    /// Antwort auf `act_grab` (oder Fehler beim Lesen der Markierung)
    pub fn grab_done(&mut self, gen: u64, res: Result<GrabInfo, String>, now: Instant) -> Vec<Effect> {
        if gen != self.gen || self.phase != Phase::Grabbing {
            return vec![];
        }
        let mut out = vec![];
        match res {
            Ok(g) => {
                // Palette: nur die Frage startet sofort (ihr Text kommt aus der Palette, der User hat ihn gesehen).
                // `text_action` und die Zwischenablage zeigen die Vorschau und warten auf die Bestätigung
                // (Entscheidung 18); eine unbekannte vorgewählte Aktion ist ein Fehler, gesperrt (Geheimnis) bleibt der
                // Sperrblock der Auswahl
                let want = match self.source {
                    Source::Question => g.actions.first().map(|a| a.name.clone()),
                    Source::Selection => self.preselect.clone(),
                    Source::Clipboard => None,
                };
                let found = want.as_ref().map(|w| g.actions.iter().find(|a| &a.name == w).map(|a| a.allowed));
                self.grab = Some(g);
                match found {
                    Some(Some(true)) if self.source == Source::Question => return self.run_action(&want.unwrap_or_default(), now),
                    Some(None) => {
                        self.err = Some(ErrInfo {
                            title: "Unknown text action".into(),
                            body: Some("Pick an action from the list instead (Mod+I).".into()),
                            neutral: false,
                            retry: false,
                            back: false,
                        });
                        self.set_phase(Phase::Error, now, &mut out);
                    }
                    _ => self.set_phase(Phase::Select, now, &mut out),
                }
            }
            Err(msg) => {
                self.err = Some(grab_error(&msg));
                self.set_phase(Phase::Error, now, &mut out);
            }
        }
        out
    }

    fn run_action(&mut self, name: &str, now: Instant) -> Vec<Effect> {
        let Some(g) = &self.grab else { return vec![] };
        let Some(a) = g.actions.iter().find(|a| a.name == name && a.allowed).cloned() else { return vec![] };
        let id = g.id.clone();
        let mut out = self.stop_run_effects();
        self.run_gen += 1;
        self.wipe_result();
        self.err = None;
        self.show_marks = true;
        self.action = Some(a.clone());
        self.running = true;
        self.run_started = Some(now);
        self.set_phase(Phase::Working, now, &mut out);
        out.push(Effect::Run { gen: self.gen, run_gen: self.run_gen, id, action: a.name });
        out
    }

    /// Ein Ereignis aus `act_run` Nummer `run_gen`
    pub fn run_event(&mut self, gen: u64, run_gen: u64, ev: RunEv, now: Instant) -> Vec<Effect> {
        if gen != self.gen || run_gen != self.run_gen || self.phase != Phase::Working {
            return vec![];
        }
        let mut out = vec![];
        match ev {
            RunEv::Delta(d) => {
                // das erste Stück ändert auch Titel und Hinweis: ganz neu zeichnen
                let first = self.text.is_empty();
                self.text.push_str(&d);
                out.push(if first { Effect::Render } else { Effect::Stream });
            }
            RunEv::Result { output, truncated, note, text, diff } => {
                self.running = false;
                self.note = note;
                wipe(&mut self.text);
                self.text = text;
                self.result_output = Some(output);
                self.truncated = truncated;
                self.diff = diff;
                self.gate.touch(now);
                self.set_phase(Phase::Result, now, &mut out);
                if std::mem::take(&mut self.chosen_pending) {
                    out.push(Effect::Chosen);
                }
            }
            // kein Fehler: zurück zur Auswahl (die Sitzung lebt)
            RunEv::Cancelled => {
                self.running = false;
                return self.back(now);
            }
            RunEv::Error(msg) => {
                self.running = false;
                self.err = Some(run_error(&msg, self.source));
                self.gate.touch(now);
                self.set_phase(Phase::Error, now, &mut out);
            }
            RunEv::Ended { daemon_down } => {
                self.running = false;
                let msg = if daemon_down { DAEMON_DOWN } else { "The action ended without an answer" };
                self.err = Some(run_error(msg, self.source));
                self.gate.touch(now);
                self.set_phase(Phase::Error, now, &mut out);
            }
        }
        out
    }

    /// Zurück zur Auswahl (Back, Stop, Abbruch); Sitzung bleibt. Eine Frage hat keine Auswahl: zurück heißt schließen
    fn back(&mut self, now: Instant) -> Vec<Effect> {
        if self.grab.is_none() || self.source == Source::Question {
            return self.close();
        }
        let mut out = self.stop_run_effects();
        self.run_gen += 1;
        self.wipe_result();
        self.action = None;
        self.err = None;
        self.gate.touch(now);
        self.set_phase(Phase::Select, now, &mut out);
        out
    }

    /// Ergebnis von Copy
    pub fn copied(&mut self, gen: u64, block: Option<usize>, res: Result<u64, String>) -> Vec<Effect> {
        if gen != self.gen {
            return vec![];
        }
        if block.is_none() {
            self.copying = false;
        }
        match (res, block) {
            (Ok(n), None) => {
                let mut out = self.close();
                out.push(Effect::Notice(format!("Copied {}", plural(n, "character", "characters"))));
                out
            }
            (Ok(_), Some(_)) => vec![],
            (Err(e), _) => {
                self.notice = Some(e);
                vec![Effect::Render]
            }
        }
    }

    /// Fokus des Panels verloren (anderes Fenster gewählt): schließen
    pub fn focus_lost(&mut self) -> Vec<Effect> {
        self.close()
    }

    /// Warum Replace nicht geht (`None` = erlaubt)
    pub fn replace_blocker(&self) -> Option<&'static str> {
        if self.result_output.as_deref() != Some("replace") {
            return Some("Not a replace action.");
        }
        if !self.grab.as_ref().is_some_and(|g| g.replace_ok) {
            return Some("Replace is off in terminals. Copy the text instead.");
        }
        if self.target.window.is_none() {
            return Some("The window is unknown, so Replace is off. Copy the text instead.");
        }
        None
    }

    fn default_active(&self) -> Option<String> {
        match self.phase {
            Phase::Result => {
                if self.result_output.as_deref() == Some("replace") && self.replace_blocker().is_none() {
                    Some("replace".into())
                } else {
                    Some("copy".into())
                }
            }
            Phase::Error => Some(if self.err.as_ref().is_some_and(|e| e.retry) { "retry".into() } else { "close_btn".into() }),
            Phase::Select if self.grab.as_ref().is_some_and(|g| !g.any_allowed() && !g.secret.is_empty()) => Some("close_btn".into()),
            // die Zwischenablage hat genau eine Aktion: Return auf ihr sendet (nach der Totzeit, nie ein gehaltenes)
            Phase::Select if self.source == Source::Clipboard => {
                self.grab.as_ref().and_then(|g| g.actions.iter().find(|a| a.allowed)).map(|a| format!("action:{}", a.name))
            }
            // `text_action` aus der Palette: die vorgewählte Aktion ist hervorgehoben, Return auf ihr sendet
            Phase::Select if self.source == Source::Selection && self.preselect.is_some() => self
                .grab
                .as_ref()
                .and_then(|g| g.actions.iter().find(|a| a.allowed && Some(&a.name) == self.preselect.as_ref()))
                .map(|a| format!("action:{}", a.name)),
            _ => None,
        }
    }

    /// Bedienbare Elemente des aktuellen Zustands in Tab-Reihenfolge (Zeilen zuerst, dann Knöpfe links, rechts)
    pub fn items(&self) -> Vec<Item> {
        let mut v = vec![];
        match self.phase {
            Phase::Select => {
                let Some(g) = &self.grab else { return v };
                if !g.any_allowed() && !g.secret.is_empty() {
                    v.push(item("settings", if g.cloud { "Open AI settings" } else { "Open settings" }, None, ItemKind::Ghost, true));
                    v.push(item("close_btn", "Close", Some("Esc"), ItemKind::Secondary, false));
                    return v;
                }
                for replace in [false, true] {
                    for a in g.actions.iter().filter(|a| a.replaces() == replace) {
                        let mut it = item(&format!("action:{}", a.name), &a.label, Some(&a.key), ItemKind::Row, false);
                        it.enabled = a.allowed;
                        v.push(it);
                    }
                }
                v.push(item("settings", "Settings", None, ItemKind::Ghost, false));
            }
            Phase::Working => v.push(item("stop", "Stop", Some("Esc"), ItemKind::Secondary, false)),
            Phase::Result => {
                let replace = self.result_output.as_deref() == Some("replace");
                if self.source != Source::Question {
                    v.push(item("back", "Back", Some("⌫"), ItemKind::Ghost, true));
                }
                if replace && !self.diff.is_empty() {
                    v.push(item("marks", if self.show_marks { "Show clean text" } else { "Show changes" }, None, ItemKind::Ghost, true));
                }
                if replace && self.replace_blocker().is_none() {
                    v.push(item("copy", "Copy", Some("c"), ItemKind::Secondary, false));
                    v.push(item("replace", "Replace", Some("Enter"), ItemKind::Primary, false));
                } else {
                    v.push(item("copy", if replace { "Copy" } else { "Copy answer" }, Some("Enter"), ItemKind::Primary, false));
                }
            }
            Phase::Error => {
                let e = self.err.as_ref();
                if e.is_some_and(|e| e.back) {
                    v.push(item("back", "Back", Some("⌫"), ItemKind::Ghost, true));
                }
                v.push(item("close_btn", "Close", Some("Esc"), ItemKind::Secondary, false));
                if e.is_some_and(|e| e.retry) {
                    v.push(item("retry", "Try again", Some("Enter"), ItemKind::Primary, false));
                }
            }
            Phase::Closed | Phase::Grabbing => {}
        }
        v
    }

    fn enabled(&self, id: &str) -> bool {
        // ✕ im Kopf: in jedem Zustand (steht nicht unter den Elementen der Tab-Reihenfolge)
        if id == "close" {
            return self.is_open();
        }
        if let Some(n) = id.strip_prefix("copy_block:") {
            return self.phase == Phase::Result && n.parse::<usize>().is_ok_and(|n| n >= 1);
        }
        self.items().iter().any(|i| i.id == id && i.enabled)
    }

    /// Element auslösen (Klick, Enter auf der Hervorhebung, Taste); in der Totzeit ignoriert
    pub fn activate(&mut self, id: &str, now: Instant) -> Vec<Effect> {
        if !self.is_open() {
            return vec![];
        }
        if !self.gate.click(now) {
            return vec![];
        }
        if !self.enabled(id) {
            return vec![];
        }
        self.gate.touch(now);
        let gid = self.grab.as_ref().map(|g| g.id.clone()).unwrap_or_default();
        if let Some(name) = id.strip_prefix("action:") {
            return self.run_action(name, now);
        }
        if let Some(n) = id.strip_prefix("copy_block:") {
            return vec![Effect::Copy { gen: self.gen, id: gid, block: n.parse().ok() }];
        }
        match id {
            "copy" if !self.copying => {
                self.copying = true;
                vec![Effect::Copy { gen: self.gen, id: gid, block: None }]
            }
            "replace" => {
                let Some(window) = self.target.window else { return vec![] };
                if self.text.is_empty() {
                    return self.close();
                }
                let text = std::mem::take(&mut self.text);
                let mut out = self.close();
                out.push(Effect::Replace { text, window });
                out
            }
            "back" => self.back(now),
            "stop" => self.back(now),
            "marks" => {
                self.show_marks = !self.show_marks;
                vec![Effect::Render]
            }
            "retry" => match self.action.clone() {
                Some(a) => self.run_action(&a.name, now),
                None => vec![],
            },
            "close_btn" | "close" => self.close(),
            "settings" => {
                let mut out = self.close();
                out.push(Effect::Settings("actions"));
                out
            }
            _ => vec![],
        }
    }

    /// Hervorhebung setzen (Hover)
    pub fn hover(&mut self, id: Option<&str>) -> Vec<Effect> {
        let id = id.filter(|i| self.enabled(i)).map(str::to_string);
        if id.is_some() && id != self.active {
            self.active = id;
            return vec![Effect::Highlight];
        }
        vec![]
    }

    fn move_active(&mut self, dir: i32, rows_only: bool, to_edge: bool) -> Vec<Effect> {
        let list: Vec<String> = self.items().into_iter().filter(|i| i.enabled && (!rows_only || i.kind == ItemKind::Row)).map(|i| i.id).collect();
        if list.is_empty() {
            return vec![];
        }
        let n = list.len() as i32;
        let idx = list.iter().position(|i| Some(i) == self.active.as_ref()).map(|i| i as i32);
        let next = if to_edge {
            if dir > 0 {
                n - 1
            } else {
                0
            }
        } else {
            match idx {
                None if dir > 0 => 0,
                None => n - 1,
                Some(i) => (i + dir).rem_euclid(n),
            }
        };
        self.active = Some(list[next as usize].clone());
        vec![Effect::Render]
    }

    /// Taste gedrückt (`key` wie `gdk::Key::name()`)
    pub fn key_press(&mut self, key: &str, mods: Mods, now: Instant) -> Vec<Effect> {
        self.gate.press(key, now);
        self.on_key(key, mods, now)
    }

    pub fn key_release(&mut self, key: &str, now: Instant) {
        self.gate.release(key, now);
    }

    /// Auslösende Taste: nach der Totzeit und nur bei neuem Druck
    fn fire(&mut self, key: &str, now: Instant) -> bool {
        self.gate.fire(key, now)
    }

    fn on_key(&mut self, key: &str, mods: Mods, now: Instant) -> Vec<Effect> {
        if !self.is_open() {
            return vec![];
        }
        self.gate.touch(now);
        if key == "Escape" {
            // während der Arbeit: abbrechen, zurück zur Auswahl (eine Frage: abbrechen und schließen); sonst schließen
            return if self.phase == Phase::Working { self.back(now) } else { self.close() };
        }
        if !mods.plain() {
            return vec![];
        }
        if key == "Tab" || key == "ISO_Left_Tab" {
            return self.move_active(if mods.shift || key == "ISO_Left_Tab" { -1 } else { 1 }, false, false);
        }
        let enter = key == "Return" || key == "KP_Enter";
        match self.phase {
            Phase::Select => {
                match key {
                    "Up" | "Down" => return self.move_active(if key == "Up" { -1 } else { 1 }, true, false),
                    "Home" | "End" => return self.move_active(if key == "Home" { -1 } else { 1 }, true, true),
                    _ => {}
                }
                if enter {
                    if let Some(a) = self.active.clone() {
                        if self.fire(key, now) {
                            return self.activate(&a, now);
                        }
                    }
                    return vec![];
                }
                let hit = self.grab.as_ref().and_then(|g| g.actions.iter().find(|a| a.key == key).map(|a| (a.name.clone(), a.allowed)));
                if let Some((name, allowed)) = hit {
                    if allowed && self.fire(key, now) {
                        return self.activate(&format!("action:{name}"), now);
                    }
                }
                vec![]
            }
            Phase::Working => {
                if self.text.is_empty() {
                    vec![]
                } else {
                    scroll_key(key).map(|s| vec![Effect::Scroll(s)]).unwrap_or_default()
                }
            }
            Phase::Result => {
                let target = if enter {
                    self.active.clone()
                } else {
                    match key {
                        "c" => Some("copy".to_string()),
                        "BackSpace" => Some("back".to_string()),
                        "d" if self.enabled("marks") => Some("marks".to_string()),
                        _ => return scroll_key(key).map(|s| vec![Effect::Scroll(s)]).unwrap_or_default(),
                    }
                };
                match target {
                    Some(t) if self.fire(key, now) => self.activate(&t, now),
                    _ => vec![],
                }
            }
            Phase::Error => {
                let target = if enter {
                    self.active.clone()
                } else if key == "BackSpace" && self.enabled("back") {
                    Some("back".to_string())
                } else {
                    None
                };
                match target {
                    Some(t) if self.fire(key, now) => self.activate(&t, now),
                    _ => vec![],
                }
            }
            Phase::Grabbing | Phase::Closed => vec![],
        }
    }
}

fn scroll_key(key: &str) -> Option<Scroll> {
    Some(match key {
        "Up" | "k" => Scroll::LineUp,
        "Down" | "j" => Scroll::LineDown,
        "Prior" | "Page_Up" => Scroll::PageUp,
        "Next" | "Page_Down" | "space" => Scroll::PageDown,
        "Home" => Scroll::Top,
        "End" => Scroll::Bottom,
        _ => return None,
    })
}

/// Meldung des Ausgabe-Threads, wenn CLIPBOARD den Hinweis eines Passwortmanagers trägt (Lesart L8)
pub const CLIPBOARD_PASSWORD: &str = "The clipboard holds a password";

/// Was der Text ist, in Meldungen
pub fn noun(source: Source) -> &'static str {
    match source {
        Source::Selection => "selection",
        Source::Clipboard => "clipboard text",
        Source::Question => "question",
    }
}

/// Satz, wie man es noch einmal versucht (Kürzel unter niri)
fn again(source: Source) -> &'static str {
    match source {
        Source::Selection => "Select the text again and press Mod+I.",
        Source::Clipboard => "Copy the text again and open the command palette (Mod+Alt+Space).",
        Source::Question => "Ask again from the command palette (Mod+Alt+Space).",
    }
}

/// Fehler des Greifens als Anleitung (UX-actions.md, Muss 7); Kürzel unter niri: Mod+I und Mod+Alt+Space
pub fn grab_error(msg: &str) -> ErrInfo {
    let e =
        |title: &str, body: Option<&str>, neutral: bool| ErrInfo { title: title.into(), body: body.map(str::to_string), neutral, retry: false, back: false };
    if msg == "Nothing selected" {
        e(msg, Some("Select text in any window, then press Mod+I."), true)
    } else if msg == DAEMON_DOWN {
        e(msg, Some("Start it with systemctl --user start dictate, then try again."), false)
    } else if msg.starts_with("Text actions are off") {
        e(msg, Some("Turn them on in Settings › Actions."), true)
    } else if msg == "The clipboard is empty" {
        e(msg, Some("Copy some text, then ask the command palette to explain the clipboard."), true)
    } else if msg == "Nothing to ask" {
        e(msg, Some("Type a question in the command palette, for example: ? what is a monad"), true)
    } else if msg.starts_with("The command palette is off") {
        e(msg, Some("Turn it on in Settings › Palette."), true)
    } else if msg == CLIPBOARD_PASSWORD {
        e(msg, Some("It comes from a password manager, so dictate does not read it."), true)
    } else {
        e(msg, None, false)
    }
}

/// Fehler einer Aktion (`run_error` in Lua)
pub fn run_error(msg: &str, source: Source) -> ErrInfo {
    let again = again(source);
    if msg == DAEMON_DOWN {
        ErrInfo {
            title: msg.into(),
            body: Some("Start it with systemctl --user start dictate, then try again.".into()),
            neutral: false,
            retry: true,
            back: false,
        }
    } else if msg.to_lowercase().contains("unknown or expired") {
        ErrInfo { title: format!("The {} expired", noun(source)), body: Some(again.into()), neutral: false, retry: false, back: false }
    } else if msg.starts_with("Blocked")
        || msg.contains("AI server changed")
        || msg.starts_with("Text actions are off")
        || msg.starts_with("The command palette is off")
    {
        ErrInfo { title: msg.into(), body: Some(again.into()), neutral: false, retry: false, back: false }
    } else {
        ErrInfo {
            title: msg.into(),
            body: Some(format!("Your {} is still here, so you can try again.", noun(source))),
            neutral: false,
            retry: true,
            back: source != Source::Question,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::gate::DEADTIME;

    fn grab(replace_ok: bool) -> GrabInfo {
        let a = |key: &str, name: &str, output: &str, allowed: bool| ActionInfo {
            key: key.into(),
            name: name.into(),
            label: name.into(),
            output: output.into(),
            allowed,
            model: "m".into(),
        };
        GrabInfo {
            id: "7".into(),
            chars: 12,
            lines: 1,
            cloud: true,
            host: "api.example".into(),
            model: "m".into(),
            secret: vec![],
            replace_ok,
            preview: "Vorschau".into(),
            actions: vec![a("e", "explain", "answer", true), a("r", "rephrase", "replace", true), a("x", "blocked", "answer", false)],
        }
    }

    const T: Target = Target { window: Some(42), app_id: None };

    /// Öffnen und Greifen; liefert den Zeitpunkt nach der Totzeit
    fn opened(p: &mut Panel, t0: Instant, replace_ok: bool) -> Instant {
        assert_eq!(p.toggle(T.clone(), t0), vec![Effect::Grab { gen: 1, source: Source::Selection }]);
        let out = p.grab_done(1, Ok(grab(replace_ok)), t0);
        assert_eq!(out, vec![Effect::Show, Effect::Render]);
        assert_eq!(p.phase, Phase::Select);
        t0 + DEADTIME
    }

    fn no_mods() -> Mods {
        Mods::default()
    }

    #[test]
    fn open_select_run_result_copy() {
        let mut p = Panel::new();
        let t0 = Instant::now();
        let t = opened(&mut p, t0, true);
        let out = p.key_press("e", no_mods(), t);
        assert_eq!(out.last(), Some(&Effect::Run { gen: 1, run_gen: 1, id: "7".into(), action: "explain".into() }));
        assert_eq!(p.phase, Phase::Working);
        assert_eq!(p.run_event(1, 1, RunEv::Delta("Hal".into()), t), vec![Effect::Render]);
        assert_eq!(p.run_event(1, 1, RunEv::Delta("lo".into()), t), vec![Effect::Stream]);
        assert_eq!(p.text, "Hallo");
        let out = p.run_event(1, 1, RunEv::Result { output: "answer".into(), truncated: false, note: None, text: "Hallo".into(), diff: vec![] }, t);
        assert!(out.contains(&Effect::Render));
        assert_eq!(p.phase, Phase::Result);
        assert_eq!(p.active.as_deref(), Some("copy"));
        // Enter in der Totzeit nach dem Wechsel: nichts
        assert!(p.key_press("Return", no_mods(), t + Duration::from_millis(10)).is_empty());
        p.key_release("Return", t + Duration::from_millis(20));
        let t2 = t + DEADTIME + Duration::from_millis(10);
        assert_eq!(p.key_press("Return", no_mods(), t2), vec![Effect::Copy { gen: 1, id: "7".into(), block: None }]);
        // kein zweites Copy, solange eins läuft
        p.key_release("Return", t2);
        assert!(p.key_press("c", no_mods(), t2 + Duration::from_millis(100)).is_empty());
        let out = p.copied(1, None, Ok(5));
        assert!(out.contains(&Effect::Clear) && out.contains(&Effect::Hide), "{out:?}");
        assert_eq!(out.last(), Some(&Effect::Notice("Copied 5 characters".into())));
        assert_eq!(p.phase, Phase::Closed);
        assert!(p.text.is_empty());
    }

    #[test]
    fn close_button_in_the_header_works_in_every_state() {
        let mut p = Panel::new();
        let t0 = Instant::now();
        assert!(p.activate("close", t0).is_empty(), "closed panel");
        let t = opened(&mut p, t0, true);
        let out = p.activate("close", t);
        assert!(out.contains(&Effect::Clear) && out.contains(&Effect::Hide), "{out:?}");
        assert_eq!(p.phase, Phase::Closed);
        // auch mitten in der Arbeit (bricht ab und löscht)
        let mut p = Panel::new();
        let t = opened(&mut p, t0, true);
        p.key_press("e", no_mods(), t);
        p.key_release("e", t);
        let out = p.activate("close", t + DEADTIME);
        assert!(out.iter().any(|e| matches!(e, Effect::CancelRun { .. })) && out.contains(&Effect::Clear), "{out:?}");
        // in der Totzeit nicht
        let mut p = Panel::new();
        opened(&mut p, t0, true);
        assert!(p.activate("close", t0 + Duration::from_millis(10)).is_empty());
        assert!(p.is_open());
    }

    #[test]
    fn hover_only_highlights_and_never_rebuilds() {
        let mut p = Panel::new();
        let t = opened(&mut p, Instant::now(), true);
        assert_eq!(p.hover(Some("action:explain")), vec![Effect::Highlight]);
        assert_eq!(p.active.as_deref(), Some("action:explain"));
        assert!(p.hover(Some("action:explain")).is_empty(), "same element again");
        assert!(p.hover(Some("action:blocked")).is_empty(), "disabled element");
        assert!(p.hover(None).is_empty(), "leaving keeps the highlight");
        assert_eq!(p.active.as_deref(), Some("action:explain"));
        // der Klick danach trifft
        assert!(matches!(p.activate("action:explain", t).last(), Some(Effect::Run { .. })));
    }

    #[test]
    fn late_answers_are_dropped() {
        let mut p = Panel::new();
        let t0 = Instant::now();
        let t = opened(&mut p, t0, true);
        p.key_press("e", no_mods(), t);
        // Stop: zurück zur Auswahl, laufender Aufruf wird abgebrochen
        let out = p.key_press("Escape", no_mods(), t);
        assert!(out.contains(&Effect::CancelRun { id: "7".into() }), "{out:?}");
        assert_eq!(p.phase, Phase::Select);
        // spätes Delta und Ergebnis der abgebrochenen Aktion
        assert!(p.run_event(1, 1, RunEv::Delta("alt".into()), t).is_empty());
        assert!(p.run_event(1, 1, RunEv::Result { output: "answer".into(), truncated: false, note: None, text: "alt".into(), diff: vec![] }, t).is_empty());
        assert_eq!(p.phase, Phase::Select);
        assert!(p.text.is_empty());
        // Schließen, wieder öffnen: ein spätes Greifen und ein spätes Copy der alten Sitzung zählen nicht
        p.close();
        assert_eq!(p.toggle(T.clone(), t), vec![Effect::Grab { gen: 3, source: Source::Selection }]);
        assert!(p.grab_done(1, Ok(grab(true)), t).is_empty());
        assert!(p.copied(1, None, Ok(3)).is_empty());
        assert_eq!(p.phase, Phase::Grabbing);
    }

    #[test]
    fn deadtime_and_held_keys() {
        let mut p = Panel::new();
        let t0 = Instant::now();
        p.toggle(T.clone(), t0);
        // `r` schon gedrückt, während gegriffen wird (z. B. Tippen beim Öffnen)
        p.key_press("r", no_mods(), t0);
        p.grab_done(1, Ok(grab(true)), t0);
        let t = t0 + DEADTIME * 2;
        // Wiederholung nur mit Drücken (GTK unter Wayland): bleibt alt
        assert!(p.key_press("r", no_mods(), t).is_empty());
        assert!(p.key_press("r", no_mods(), t + Duration::from_millis(30)).is_empty());
        // Wiederholung als Loslassen + Drücken binnen 50 ms (X): bleibt alt
        p.key_release("r", t + Duration::from_millis(60));
        assert!(p.key_press("r", no_mods(), t + Duration::from_millis(80)).is_empty());
        assert_eq!(p.phase, Phase::Select);
        // echtes Loslassen und neues Drücken: löst aus
        p.key_release("r", t + Duration::from_millis(100));
        let out = p.key_press("r", no_mods(), t + Duration::from_millis(300));
        assert!(matches!(out.last(), Some(Effect::Run { action, .. }) if action == "rephrase"), "{out:?}");
        assert!(p.gate.ignored >= 3);
    }

    #[test]
    fn release_just_before_a_phase_change_counts_as_repeat() {
        let mut p = Panel::new();
        let t0 = Instant::now();
        let t = opened(&mut p, t0, true);
        p.key_press("e", no_mods(), t);
        // Loslassen 20 ms vor dem Ergebnis, dann sofort wieder gedrückt (Autorepeat): `c` alt
        p.key_press("c", no_mods(), t);
        p.key_release("c", t + Duration::from_millis(100));
        let t1 = t + Duration::from_millis(120);
        p.run_event(1, 1, RunEv::Result { output: "answer".into(), truncated: false, note: None, text: "x".into(), diff: vec![] }, t1);
        // weitere Wiederholung (Loslassen und Drücken 11 ms auseinander) nach der Totzeit: bleibt alt
        p.key_press("c", no_mods(), t1 + Duration::from_millis(5));
        p.key_release("c", t1 + DEADTIME - Duration::from_millis(10));
        assert!(p.key_press("c", no_mods(), t1 + DEADTIME + Duration::from_millis(1)).is_empty());
        // neuer Druck nach echtem Loslassen: kopiert
        p.key_release("c", t1 + DEADTIME + Duration::from_millis(20));
        let out = p.key_press("c", no_mods(), t1 + DEADTIME + Duration::from_millis(200));
        assert_eq!(out, vec![Effect::Copy { gen: 1, id: "7".into(), block: None }]);
    }

    #[test]
    fn modifiers_are_not_action_keys() {
        let mut p = Panel::new();
        let t0 = Instant::now();
        let t = opened(&mut p, t0, true);
        // Super noch gehalten: Mod+e ist kein `e`
        assert!(p.key_press("e", Mods { logo: true, ..Mods::default() }, t).is_empty());
        assert!(p.key_press("e", Mods { ctrl: true, ..Mods::default() }, t).is_empty());
        assert_eq!(p.phase, Phase::Select);
        // gesperrte Aktion löst nicht aus
        assert!(p.key_press("x", no_mods(), t).is_empty());
        // Pfeile wählen nur erlaubte Zeilen, Enter löst die Hervorhebung aus
        p.key_press("Down", no_mods(), t);
        assert_eq!(p.active.as_deref(), Some("action:explain"));
        p.key_press("Down", no_mods(), t);
        assert_eq!(p.active.as_deref(), Some("action:rephrase"));
        p.key_press("Down", no_mods(), t);
        assert_eq!(p.active.as_deref(), Some("action:explain"));
        let out = p.key_press("Return", no_mods(), t);
        assert!(matches!(out.last(), Some(Effect::Run { action, .. }) if action == "explain"));
    }

    #[test]
    fn replace_needs_a_known_window_and_no_terminal() {
        let mut p = Panel::new();
        let t0 = Instant::now();
        let t = opened(&mut p, t0, true);
        p.key_press("r", no_mods(), t);
        let res = |text: &str| RunEv::Result {
            output: "replace".into(),
            truncated: false,
            note: None,
            text: text.into(),
            diff: vec![DiffPiece { op: '+', text: text.into() }],
        };
        p.run_event(1, 1, res("Neu"), t);
        assert_eq!(p.active.as_deref(), Some("replace"));
        assert!(p.items().iter().any(|i| i.id == "marks"));
        let t2 = t + DEADTIME;
        let out = p.key_press("Return", no_mods(), t2);
        assert_eq!(out.last(), Some(&Effect::Replace { text: "Neu".into(), window: 42 }));
        assert!(out.contains(&Effect::Clear) && out.contains(&Effect::Hide));
        assert_eq!(p.phase, Phase::Closed);

        // Terminal (replace_ok false): nur Copy, Enter kopiert
        let mut p = Panel::new();
        let t = opened(&mut p, t0, false);
        p.key_press("r", no_mods(), t);
        p.run_event(1, 1, res("Neu"), t);
        assert!(p.replace_blocker().unwrap().contains("terminals"));
        assert!(!p.items().iter().any(|i| i.id == "replace"));
        assert!(p.activate("replace", t + DEADTIME).is_empty());
        assert_eq!(p.active.as_deref(), Some("copy"));

        // Fenster beim Öffnen unbekannt: kein Replace
        let mut p = Panel::new();
        p.toggle(Target::default(), t0);
        p.grab_done(1, Ok(grab(true)), t0);
        p.key_press("r", no_mods(), t0 + DEADTIME);
        p.run_event(1, 1, res("Neu"), t0 + DEADTIME);
        assert!(p.replace_blocker().unwrap().contains("unknown"));
        assert!(p.activate("replace", t0 + DEADTIME * 3).is_empty());
    }

    #[test]
    fn errors_retry_and_back() {
        let mut p = Panel::new();
        let t0 = Instant::now();
        p.toggle(T.clone(), t0);
        let out = p.grab_done(1, Err("Nothing selected".into()), t0);
        assert!(out.contains(&Effect::Show));
        assert_eq!(p.phase, Phase::Error);
        assert!(p.err.as_ref().unwrap().neutral);
        assert_eq!(p.active.as_deref(), Some("close_btn"));
        // Enter schließt (nach der Totzeit)
        let out = p.key_press("Return", no_mods(), t0 + DEADTIME);
        assert!(out.contains(&Effect::Clear));

        let mut p = Panel::new();
        let t = opened(&mut p, t0, true);
        p.key_press("e", no_mods(), t);
        p.run_event(1, 1, RunEv::Error("HTTP 529: overloaded".into()), t);
        let e = p.err.clone().unwrap();
        assert!(e.retry && e.back);
        assert_eq!(p.active.as_deref(), Some("retry"));
        let out = p.key_press("Return", no_mods(), t + DEADTIME);
        assert_eq!(out.last(), Some(&Effect::Run { gen: 1, run_gen: 2, id: "7".into(), action: "explain".into() }));
        p.run_event(1, 2, RunEv::Ended { daemon_down: true }, t);
        assert_eq!(p.err.as_ref().unwrap().title, DAEMON_DOWN);
        assert!(!p.items().iter().any(|i| i.id == "back"));
        p.key_release("Return", t + DEADTIME);
        let t3 = t + DEADTIME * 3;
        let out = p.key_press("Return", no_mods(), t3);
        assert_eq!(out.last(), Some(&Effect::Run { gen: 1, run_gen: 3, id: "7".into(), action: "explain".into() }));
        p.run_event(1, 3, RunEv::Error("boom".into()), t3);
        let out = p.key_press("BackSpace", no_mods(), t3 + DEADTIME);
        assert!(out.contains(&Effect::Render));
        assert_eq!(p.phase, Phase::Select);
        // abgelaufene Sitzung: kein Retry
        assert!(!run_error("Unknown or expired selection", Source::Selection).retry);
    }

    #[test]
    fn cancelled_goes_back_and_close_cancels() {
        let mut p = Panel::new();
        let t0 = Instant::now();
        let t = opened(&mut p, t0, true);
        p.key_press("e", no_mods(), t);
        p.run_event(1, 1, RunEv::Cancelled, t);
        assert_eq!(p.phase, Phase::Select);
        p.key_release("e", t);
        p.key_press("e", no_mods(), t + DEADTIME * 2);
        assert_eq!(p.phase, Phase::Working);
        // Fokusverlust während der Arbeit: abbrechen und löschen
        let out = p.focus_lost();
        assert_eq!(out, vec![Effect::CancelRun { id: "7".into() }, Effect::Clear, Effect::Hide]);
        assert!(p.close().is_empty());
    }

    #[test]
    fn show_delay_and_idle() {
        let mut p = Panel::new();
        let t0 = Instant::now();
        p.toggle(T.clone(), t0);
        assert_eq!(p.next_deadline(), Some(t0 + SHOW_DELAY));
        assert!(p.tick(t0 + Duration::from_millis(100)).is_empty());
        assert_eq!(p.tick(t0 + SHOW_DELAY), vec![Effect::Show, Effect::Render]);
        // danach kein zweites Show
        let out = p.grab_done(1, Ok(grab(true)), t0 + SHOW_DELAY);
        assert_eq!(out, vec![Effect::Render]);
        assert_eq!(p.next_deadline(), Some(t0 + IDLE));
        p.key_press("Down", no_mods(), t0 + Duration::from_secs(10));
        assert_eq!(p.next_deadline(), Some(t0 + Duration::from_secs(10) + IDLE));
        assert!(p.tick(t0 + IDLE).is_empty());
        let out = p.tick(t0 + Duration::from_secs(10) + IDLE);
        assert!(out.contains(&Effect::Clear));
        assert_eq!(p.phase, Phase::Closed);
        // Mod+I zweimal: zu
        p.toggle(T.clone(), t0);
        assert!(p.toggle(T.clone(), t0).contains(&Effect::Clear));
    }

    #[test]
    fn preview_limits_lines_and_chars() {
        assert_eq!(preview("\n\n  a\r\nb\tc\u{1b}[0m\n").as_str(), "a\nb\tc[0m");
        let many = (1..=10).map(|i| format!("l{i}")).collect::<Vec<_>>().join("\n");
        assert_eq!(preview(&many).as_str(), "l1\nl2\nl3\nl4\nl5\nl6");
        assert_eq!(preview(&"ä".repeat(1000)).as_str().chars().count(), PREVIEW_CHARS);
    }

    #[test]
    fn secret_block_and_grab_json() {
        let v = serde_json::json!({"ok":true,"id":"9","chars":3,"lines":1,"target":{"host":"h","model":"m","cloud":true},
            "secret":["private_key"],"blocked":true,"replace_ok":false,
            "actions":[{"key":"e","name":"explain","label":"Explain","output":"answer","allowed":false,"model":"m"}]});
        let g = GrabInfo::from_json(&v, "p".into()).unwrap();
        assert_eq!(g.secret, ["private_key"]);
        assert!(!g.any_allowed() && g.cloud);
        assert!(GrabInfo::from_json(&serde_json::json!({"ok":true}), String::new()).is_none());
        let mut p = Panel::new();
        let t0 = Instant::now();
        p.toggle(T.clone(), t0);
        p.grab_done(1, Ok(g), t0);
        let ids: Vec<String> = p.items().into_iter().map(|i| i.id).collect();
        assert_eq!(ids, ["settings", "close_btn"]);
        assert_eq!(p.active.as_deref(), Some("close_btn"));
        let out = p.activate("settings", t0 + DEADTIME);
        assert_eq!(out.last(), Some(&Effect::Settings("actions")));
    }
    fn result(text: &str) -> RunEv {
        RunEv::Result { output: "answer".into(), truncated: false, note: None, text: text.into(), diff: vec![] }
    }

    /// Antwort auf `act_grab` einer Quelle der Palette: eine Aktion
    fn one(name: &str, allowed: bool) -> GrabInfo {
        let mut g = grab(false);
        g.actions = vec![ActionInfo { key: "a".into(), name: name.into(), label: name.into(), output: "answer".into(), allowed, model: "m".into() }];
        g
    }

    #[test]
    fn question_starts_at_once_and_has_no_way_back() {
        let mut p = Panel::new();
        let t0 = Instant::now();
        let out = p.open_with(Source::Question, None, Some("Was ist ein Monad?".into()), T.clone(), t0);
        assert_eq!(out, vec![Effect::Grab { gen: 1, source: Source::Question }]);
        assert_eq!(p.question, "Was ist ein Monad?");
        let out = p.grab_done(1, Ok(one("ask", true)), t0);
        assert_eq!(out.last(), Some(&Effect::Run { gen: 1, run_gen: 1, id: "7".into(), action: "ask".into() }));
        assert_eq!(p.phase, Phase::Working);
        let out = p.run_event(1, 1, result("Antwort"), t0);
        // das erste Ergebnis meldet die Wahl der Palette, genau einmal
        assert_eq!(out.iter().filter(|e| **e == Effect::Chosen).count(), 1);
        assert!(!p.items().iter().any(|i| i.id == "back"), "a question has no list to go back to");
        // ein zweiter Lauf (Retry gibt es hier nicht, also direkt) meldet nichts mehr
        let t = t0 + DEADTIME * 2;
        assert!(p.activate("back", t).is_empty());
        // Esc während der Arbeit: abbrechen und schließen (nicht zurück zu einer Auswahl)
        let mut p = Panel::new();
        p.open_with(Source::Question, None, Some("Frage".into()), T.clone(), t0);
        p.grab_done(1, Ok(one("ask", true)), t0);
        let out = p.key_press("Escape", no_mods(), t0 + DEADTIME);
        assert!(out.contains(&Effect::CancelRun { id: "7".into() }) && out.contains(&Effect::Clear) && out.contains(&Effect::Hide));
        assert!(!p.is_open());
        assert!(p.question.is_empty(), "the question is wiped on close");
        // Fehler: Anleitung zur Palette, kein Back
        let mut p = Panel::new();
        p.open_with(Source::Question, None, Some("Frage".into()), T.clone(), t0);
        p.grab_done(1, Ok(one("ask", true)), t0);
        p.run_event(1, 1, RunEv::Error("boom".into()), t0);
        let e = p.err.clone().unwrap();
        assert!(e.retry && !e.back);
        assert_eq!(e.body.as_deref(), Some("Your question is still here, so you can try again."));
        assert!(!p.items().iter().any(|i| i.id == "back"));
        assert!(run_error("Unknown or expired selection", Source::Question).body.unwrap().contains("Mod+Alt+Space"));
    }

    #[test]
    fn question_with_a_secret_stays_blocked_without_chosen() {
        let mut p = Panel::new();
        let t0 = Instant::now();
        p.open_with(Source::Question, None, Some("Frage".into()), T.clone(), t0);
        let mut g = one("ask", false);
        g.secret = vec!["private_key".into()];
        let out = p.grab_done(1, Ok(g), t0);
        assert!(!out.iter().any(|e| matches!(e, Effect::Run { .. } | Effect::Chosen)));
        assert_eq!(p.phase, Phase::Select);
        assert_eq!(p.items().into_iter().map(|i| i.id).collect::<Vec<_>>(), ["settings", "close_btn"]);
    }

    #[test]
    fn clipboard_waits_for_the_confirmation() {
        let mut p = Panel::new();
        let t0 = Instant::now();
        let out = p.open_with(Source::Clipboard, Some("explain"), Some("ignored".into()), T.clone(), t0);
        assert_eq!(out, vec![Effect::Grab { gen: 1, source: Source::Clipboard }]);
        assert!(p.question.is_empty(), "only a question carries text from the palette");
        let out = p.grab_done(1, Ok(one("explain", true)), t0);
        assert!(!out.iter().any(|e| matches!(e, Effect::Run { .. })), "nothing is sent before the preview is confirmed");
        assert_eq!(p.phase, Phase::Select);
        // Esc in der Vorschau sendet nichts
        let out = p.key_press("Escape", no_mods(), t0 + DEADTIME);
        assert!(!out.iter().any(|e| matches!(e, Effect::Run { .. })));
        assert!(!p.is_open());
        // Return in der Totzeit nicht, danach auf der hervorgehobenen Aktion schon
        let mut p = Panel::new();
        p.open_with(Source::Clipboard, None, None, T.clone(), t0);
        p.grab_done(1, Ok(one("explain", true)), t0);
        assert_eq!(p.active.as_deref(), Some("action:explain"), "the one action is highlighted: Return sends");
        assert!(p.key_press("Return", no_mods(), t0 + Duration::from_millis(10)).is_empty());
        p.key_release("Return", t0 + Duration::from_millis(20));
        let out = p.key_press("Return", no_mods(), t0 + DEADTIME);
        assert_eq!(out.last(), Some(&Effect::Run { gen: 1, run_gen: 1, id: "7".into(), action: "explain".into() }));
        let out = p.run_event(1, 1, result("Erklärung"), t0 + DEADTIME);
        assert!(out.contains(&Effect::Chosen));
        // Back zur Vorschau, zweiter Lauf: keine zweite Meldung
        p.activate("back", t0 + DEADTIME * 3);
        let out = p.key_press("a", no_mods(), t0 + DEADTIME * 5);
        assert_eq!(out.last(), Some(&Effect::Run { gen: 1, run_gen: 3, id: "7".into(), action: "explain".into() }));
        assert!(!p.run_event(1, 3, result("noch einmal"), t0 + DEADTIME * 5).contains(&Effect::Chosen));
        assert!(grab_error(CLIPBOARD_PASSWORD).neutral);
        assert_eq!(run_error("Unknown or expired selection", Source::Clipboard).title, "The clipboard text expired");
    }

    #[test]
    fn preselect_waits_for_the_confirmation_on_the_selection() {
        let mut p = Panel::new();
        let t0 = Instant::now();
        let out = p.open_with(Source::Selection, Some("rephrase"), None, T.clone(), t0);
        assert_eq!(out, vec![Effect::Grab { gen: 1, source: Source::Selection }]);
        // Entscheidung 18: nichts geht ungesehen an die KI, die vorgewählte Aktion ist hervorgehoben
        let out = p.grab_done(1, Ok(grab(true)), t0);
        assert!(!out.iter().any(|e| matches!(e, Effect::Run { .. })), "nothing is sent before the preview is confirmed");
        assert_eq!(p.phase, Phase::Select);
        assert_eq!(p.active.as_deref(), Some("action:rephrase"), "the preselected action is highlighted");
        // Return in der Totzeit nicht, danach auf der hervorgehobenen Aktion schon
        assert!(p.key_press("Return", no_mods(), t0 + Duration::from_millis(10)).is_empty());
        p.key_release("Return", t0 + Duration::from_millis(20));
        let out = p.key_press("Return", no_mods(), t0 + DEADTIME);
        assert_eq!(out.last(), Some(&Effect::Run { gen: 1, run_gen: 1, id: "7".into(), action: "rephrase".into() }));
        // Esc in der Vorschau sendet nichts
        let mut q = Panel::new();
        q.open_with(Source::Selection, Some("rephrase"), None, T.clone(), t0);
        q.grab_done(1, Ok(grab(true)), t0);
        let out = q.key_press("Escape", no_mods(), t0 + DEADTIME);
        assert!(!out.iter().any(|e| matches!(e, Effect::Run { .. })));
        assert!(!q.is_open());
        // Back führt zur Auswahl (Mod+I-Ablauf)
        p.run_event(1, 1, RunEv::Result { output: "replace".into(), truncated: false, note: None, text: "neu".into(), diff: vec![] }, t0);
        assert!(p.items().iter().any(|i| i.id == "back"));
        p.activate("back", t0 + DEADTIME);
        assert_eq!(p.phase, Phase::Select);
        // gesperrte Aktion: Auswahl statt Lauf; unbekannte Aktion: Fehler mit Anleitung
        let mut p = Panel::new();
        p.open_with(Source::Selection, Some("blocked"), None, T.clone(), t0);
        let out = p.grab_done(1, Ok(grab(true)), t0);
        assert!(!out.iter().any(|e| matches!(e, Effect::Run { .. })));
        assert_eq!(p.phase, Phase::Select);
        let mut p = Panel::new();
        p.open_with(Source::Selection, Some("nope"), None, T.clone(), t0);
        p.grab_done(1, Ok(grab(true)), t0);
        assert_eq!(p.phase, Phase::Error);
        assert_eq!(p.err.as_ref().unwrap().title, "Unknown text action");
        // Mod+I danach: wieder ohne Vorwahl und ohne Meldung an die Palette
        p.close();
        p.toggle(T.clone(), t0);
        let out = p.grab_done(p.gen(), Ok(grab(true)), t0);
        assert!(!out.iter().any(|e| matches!(e, Effect::Run { .. })));
        p.activate("action:explain", t0 + DEADTIME);
        assert!(!p.run_event(p.gen(), p.run_gen(), result("x"), t0 + DEADTIME).contains(&Effect::Chosen));
    }

    #[test]
    fn open_with_closes_an_open_panel_first() {
        let mut p = Panel::new();
        let t0 = Instant::now();
        opened(&mut p, t0, true);
        let out = p.open_with(Source::Clipboard, None, None, T.clone(), t0);
        assert!(out.contains(&Effect::Clear) && out.contains(&Effect::Hide));
        assert_eq!(out.last(), Some(&Effect::Grab { gen: 3, source: Source::Clipboard }));
        // eine späte Antwort der alten Öffnung zählt nicht
        assert!(p.grab_done(1, Ok(grab(true)), t0).is_empty());
    }
}
