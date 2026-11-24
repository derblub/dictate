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

//! Schlüsselwörter und Befehlspräfixe des lokalen Resolvers (Stufe 1 in `PLAN-palette.md`) als Daten.
//!
//! - [`KEYWORDS`]: feste Absichten ohne Parameter (oder mit festem Parameter), je mit Wendungen auf Deutsch und
//!   Englisch. Der Resolver faltet sie wie jeden Text (Groß/klein und Umlaute egal: „schließen“, „schliessen“) und
//!   bewertet die Eingabe mit demselben Fuzzy-Verfahren; „sperr“ trifft „sperren“ als Präfix.
//! - [`PREFIXES`]: erste Wörter, die einen Parameter ankündigen („volume 40“, „g rust“, „starte firefox“, „verschiebe
//!   nach 3“). Der Rest der Eingabe nach dem Präfix wird je [`Action`] ausgewertet.
//!
//! Eine neue Wendung ist eine neue Zeile in der Tabelle, keine neue Verzweigung im Code. Die Anzeigetexte sind englisch
//! (sichtbare Texte des Tools), die Wendungen deutsch und englisch.

use crate::palette::intent::{ColumnDisplay, DiagProfile, Intent, MediaKey, ScreenshotMode, Widget, WindowToggle};

/// Schrittweite von „lauter“/„leiser“ in Prozentpunkten, wie die Lautstärketasten in `rc.lua` (`adjust_perc(±5)`).
pub const VOLUME_STEP: i8 = 5;

/// Eine feste Absicht mit ihren Wendungen.
pub struct Keyword {
    /// Eindeutige Kennung innerhalb der Absicht; Ziel-ID der Häufigkeit (`volume:up`).
    pub id: &'static str,
    /// Wendungen (Groß/klein und Umlaute egal). Die erste ist die Hauptwendung.
    pub phrases: &'static [&'static str],
    /// Englische Anzeigezeile.
    pub display: &'static str,
    pub intent: fn() -> Intent,
    /// Kleiner Aufschlag (0 bis 1) für die Reihenfolge bei gleicher Güte (z. B. Screenshot: Bereich vor Vollbild).
    pub rank: f32,
}

const fn kw(id: &'static str, display: &'static str, phrases: &'static [&'static str], intent: fn() -> Intent) -> Keyword {
    Keyword { id, phrases, display, intent, rank: 0.0 }
}

const fn ranked(rank: f32, mut k: Keyword) -> Keyword {
    k.rank = rank;
    k
}

pub static KEYWORDS: &[Keyword] = &[
    // System
    kw("lock", "Lock screen", &["lock", "lock screen", "lock the screen", "sperren", "bildschirm sperren", "sperrbildschirm", "bildschirmsperre"], || {
        Intent::Lock {}
    }),
    kw("up", "Volume up", &["lauter", "louder", "volume up", "vol up", "turn up", "lautstärke erhöhen", "lautstärke hoch", "lautstärke lauter"], || {
        Intent::Volume { set: None, step: Some(VOLUME_STEP), mute: None }
    }),
    kw(
        "down",
        "Volume down",
        &["leiser", "quieter", "softer", "volume down", "vol down", "turn down", "lautstärke verringern", "lautstärke senken", "lautstärke runter"],
        || Intent::Volume { set: None, step: Some(-VOLUME_STEP), mute: None },
    ),
    kw("mute", "Mute", &["mute", "stumm", "stumm schalten", "stummschalten", "ton aus"], || Intent::Volume { set: None, step: None, mute: Some(true) }),
    kw("unmute", "Unmute", &["unmute", "ton an", "stumm aufheben", "stummschaltung aufheben"], || Intent::Volume { set: None, step: None, mute: Some(false) }),
    kw("play_pause", "Play / pause", &["pause", "play", "play pause", "pausieren", "wiedergabe", "abspielen", "resume", "fortsetzen"], || Intent::Media {
        action: MediaKey::PlayPause,
    }),
    kw("next", "Next track", &["next", "next track", "next song", "skip", "nächster titel", "nächstes lied", "nächster song", "weiter"], || Intent::Media {
        action: MediaKey::Next,
    }),
    kw("previous", "Previous track", &["previous track", "previous song", "prev track", "vorheriger titel", "vorheriges lied", "vorheriger song"], || {
        Intent::Media { action: MediaKey::Previous }
    }),
    ranked(
        2.0,
        kw(
            "area",
            "Screenshot · Area",
            &["screenshot", "screenshot area", "screenshot bereich", "bildschirmfoto", "bildschirmfoto bereich", "ausschnitt"],
            || Intent::Screenshot { mode: ScreenshotMode::Area },
        ),
    ),
    ranked(
        1.0,
        kw(
            "full",
            "Screenshot · Full screen",
            &["screenshot full", "screenshot full screen", "screenshot ganzer bildschirm", "screenshot vollbild", "bildschirmfoto komplett"],
            || Intent::Screenshot { mode: ScreenshotMode::Full },
        ),
    ),
    kw("window", "Screenshot · Window", &["screenshot window", "screenshot fenster", "bildschirmfoto fenster"], || Intent::Screenshot {
        mode: ScreenshotMode::Window,
    }),
    kw(
        "clipboard_history",
        "Clipboard history",
        &["clipboard", "clipboard history", "zwischenablage", "zwischenablage verlauf", "verlauf zwischenablage", "clipboard verlauf"],
        || Intent::ShowWidget { widget: Widget::ClipboardHistory },
    ),
    kw("emoji", "Emoji picker", &["emoji", "emojis", "emoji picker", "smiley", "smileys"], || Intent::ShowWidget { widget: Widget::Emoji }),
    kw("settings", "Dictate settings", &["settings", "dictate settings", "einstellungen", "dictate einstellungen", "preferences", "optionen"], || {
        Intent::DictateSettings { page: None }
    }),
    kw("logout", "Log out", &["logout", "log out", "logoff", "sign out", "abmelden", "ausloggen"], || Intent::Logout {}),
    kw(
        "reboot",
        "Restart computer",
        &["reboot", "restart", "restart computer", "neustart", "neu starten", "rechner neu starten", "computer neu starten"],
        || Intent::Reboot {},
    ),
    kw("poweroff", "Shut down", &["shutdown", "shut down", "poweroff", "power off", "ausschalten", "herunterfahren", "runterfahren"], || Intent::Poweroff {}),
    kw("suspend", "Suspend", &["suspend", "sleep", "bereitschaft", "standby", "ruhezustand", "schlafen"], || Intent::Suspend {}),
    // Fenster und Arbeitsflächen („tag“ bleibt als Wort aus Awesome erhalten, Vorlage E2 c)
    kw(
        "previous_workspace",
        "Previous workspace",
        &[
            "previous workspace",
            "vorherige arbeitsfläche",
            "letzte arbeitsfläche",
            "arbeitsfläche zurück",
            "previous tag",
            "vorheriger tag",
            "last tag",
            "letzter tag",
            "back",
            "go back",
            "zurück",
            "tag zurück",
        ],
        || Intent::PreviousWorkspace {},
    ),
    kw("floating", "Toggle floating", &["floating", "float", "toggle floating", "schwebend", "freischwebend", "schweben"], || Intent::ToggleWindow {
        what: WindowToggle::Floating,
    }),
    kw("fullscreen", "Toggle fullscreen", &["fullscreen", "full screen", "toggle fullscreen", "vollbild", "vollbildmodus"], || Intent::ToggleWindow {
        what: WindowToggle::Fullscreen,
    }),
    kw("maximized", "Toggle maximized", &["maximize", "maximise", "maximized", "toggle maximized", "maximieren", "maximiert"], || Intent::ToggleWindow {
        what: WindowToggle::Maximized,
    }),
    kw(
        "tabbed",
        "Column · Tabbed",
        &["tabbed", "tabs", "tabbed column", "column tabbed", "spalte tabs", "spalte als tabs", "reiter", "spalte reiter"],
        || Intent::ColumnDisplay { display: ColumnDisplay::Tabbed },
    ),
    kw("normal", "Column · Normal", &["normal column", "column normal", "untabbed", "no tabs", "spalte normal", "keine tabs", "reiter aus"], || {
        Intent::ColumnDisplay { display: ColumnDisplay::Normal }
    }),
    kw("close", "Close window", &["close", "close window", "schließen", "fenster schließen", "schließe", "quit window"], || Intent::CloseWindow {
        query: None,
    }),
    // Text und KI
    kw(
        "explain_clipboard",
        "Explain clipboard",
        &[
            "explain clipboard",
            "explain the clipboard",
            "explain my clipboard",
            "erkläre die zwischenablage",
            "erkläre zwischenablage",
            "zwischenablage erklären",
            "clipboard erklären",
        ],
        || Intent::ExplainClipboard {},
    ),
    // Diagnose (Klasse `ai`): öffnet das Diagnose-Panel, optional auf einem Profil; die Palette führt nie selbst eine Probe aus.
    // Nur angeboten, solange `[diagnose] enabled` gilt (`Features`)
    kw(
        "diagnose",
        "Diagnose this computer",
        &[
            "diagnose",
            "diagnosis",
            "diagnose this computer",
            "diagnose my computer",
            "diagnose the computer",
            "diagnose the machine",
            "diagnose rechner",
            "diagnose computer",
            "rechner diagnose",
            "computer diagnose",
            "linux diagnose",
            "system diagnose",
            "diagnostik",
            "diagnostizieren",
            "diagnose starten",
            "diagnosiere",
            "diagnosiere den rechner",
            "troubleshoot",
            "troubleshooting",
            "fehlersuche",
            "fehler suchen",
            "warum",
            "why",
            "what is wrong",
            "what went wrong",
            "was ist los",
            "was stimmt nicht",
        ],
        || Intent::Diagnose { profile: None, unit: None, container: None },
    ),
    kw(
        "last_crash",
        "Diagnose · Last crash",
        &[
            "crash",
            "absturz",
            "last crash",
            "letzter absturz",
            "diagnose crash",
            "diagnose absturz",
            "diagnose last crash",
            "diagnose letzter absturz",
            "why did it crash",
            "why did the computer crash",
            "why did my computer crash",
            "why did it freeze",
            "why did the computer freeze",
            "what happened at the last boot",
            "warum abgestürzt",
            "warum ist der rechner abgestürzt",
            "warum ist der computer abgestürzt",
            "warum ist er abgestürzt",
            "warum eingefroren",
            "warum hing der rechner",
            "freeze",
            "freezes",
            "eingefroren",
            "rechner eingefroren",
            "computer eingefroren",
            "unclean shutdown",
            "unsauber beendet",
            "boot crash",
        ],
        || Intent::Diagnose { profile: Some(DiagProfile::LastCrash), unit: None, container: None },
    ),
    kw(
        "now",
        "Diagnose · Right now",
        &[
            "diagnose now",
            "diagnose jetzt",
            "diagnose current state",
            "diagnose aktueller zustand",
            "health check",
            "system check",
            "systemcheck",
            "system health",
            "computer health",
            "check the system",
            "rechner prüfen",
            "system prüfen",
        ],
        || Intent::Diagnose { profile: Some(DiagProfile::Now), unit: None, container: None },
    ),
    kw(
        "unit",
        "Diagnose · Failed unit",
        &[
            "failed",
            "failed unit",
            "failed units",
            "failed service",
            "failed services",
            "service failed",
            "unit failed",
            "diagnose unit",
            "diagnose service",
            "diagnose failed",
            "diagnose dienst",
            "fehlgeschlagen",
            "fehlgeschlagene units",
            "fehlgeschlagene dienste",
            "dienst fehlgeschlagen",
            "dienst kaputt",
            "systemd fehler",
            "systemd failed",
        ],
        || Intent::Diagnose { profile: Some(DiagProfile::Unit), unit: None, container: None },
    ),
    kw(
        "docker",
        "Diagnose · Docker",
        &[
            "diagnose docker",
            "diagnose container",
            "diagnose containers",
            "docker problem",
            "docker problems",
            "docker kaputt",
            "docker broken",
            "docker problem diagnose",
            "warum ist docker kaputt",
            "why is docker broken",
            "container problem",
            "container problems",
            "container crashed",
            "container kaputt",
            "container diagnose",
        ],
        || Intent::Diagnose { profile: Some(DiagProfile::Docker), unit: None, container: None },
    ),
    kw(
        "network",
        "Diagnose · Network",
        &[
            "diagnose network",
            "diagnose netzwerk",
            "diagnose internet",
            "network problem",
            "network problems",
            "netzwerkproblem",
            "netzwerkprobleme",
            "network down",
            "no internet",
            "kein internet",
            "internet kaputt",
            "internet down",
            "warum kein internet",
            "why no internet",
            "dns problem",
            "dns kaputt",
        ],
        || Intent::Diagnose { profile: Some(DiagProfile::Network), unit: None, container: None },
    ),
    kw(
        "heat",
        "Diagnose · Heat",
        &[
            "diagnose heat",
            "diagnose temperature",
            "diagnose temperatur",
            "diagnose hitze",
            "overheating",
            "overheated",
            "too hot",
            "zu heiß",
            "zu heiss",
            "überhitzt",
            "überhitzung",
            "temperature problem",
            "temperaturproblem",
            "computer is hot",
            "rechner ist heiß",
            "lüfter laut",
            "fan loud",
        ],
        || Intent::Diagnose { profile: Some(DiagProfile::Heat), unit: None, container: None },
    ),
];

/// Was nach einem Befehlspräfix mit dem Rest der Eingabe geschieht.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Rest ist der Suchbegriff (`web_search`).
    WebSearch,
    /// Rest ist eine Zahl 0 bis 100, optional mit `%` (`volume` mit `set`).
    VolumeSet,
    /// Rest ist eine Arbeitsfläche (Zahl auf dem Monitor des Fensters oder Name); das fokussierte Fenster wandert dorthin.
    MoveToWorkspace,
    /// Rest ist eine Arbeitsfläche (Zahl auf dem fokussierten Monitor oder Name).
    ViewWorkspace,
    /// Rest sucht ein Projekt (`open_project`).
    Project,
    /// Rest sucht ein Projekt oder ein Programm, oder ist eine Adresse (`open …`).
    OpenAny,
    /// Rest sucht ein Projekt, dessen Ordner geöffnet wird (`open_folder`).
    Folder,
    /// Rest sucht ein Programm, das neu gestartet wird (`launch_app`).
    App,
    /// Rest sucht ein laufendes Fenster oder eine Arbeitsfläche (`focus_window`, `view_workspace`).
    Switch,
    /// Rest sucht ein laufendes Fenster, das geschlossen wird (`close_window`, bestätigen).
    Close,
    /// Rest ist der Suchbegriff einer Unit für die Diagnose (`diagnose` mit `unit`); aufgelöst wird er später.
    DiagnoseUnit,
    /// Rest ist der Suchbegriff eines Docker-Containers für die Diagnose (`diagnose` mit `container`).
    DiagnoseContainer,
}

/// Ein Befehlspräfix: die ersten Wörter der Eingabe (Groß/klein und Umlaute egal) und was sie bedeuten.
pub struct Prefix {
    pub words: &'static [&'static str],
    pub action: Action,
}

const fn p(words: &'static [&'static str], action: Action) -> Prefix {
    Prefix { words, action }
}

pub static PREFIXES: &[Prefix] = &[
    p(&["g"], Action::WebSearch),
    p(&["search"], Action::WebSearch),
    p(&["suche"], Action::WebSearch),
    p(&["websuche"], Action::WebSearch),
    p(&["volume"], Action::VolumeSet),
    p(&["volume", "to"], Action::VolumeSet),
    p(&["set", "volume"], Action::VolumeSet),
    p(&["set", "volume", "to"], Action::VolumeSet),
    p(&["vol"], Action::VolumeSet),
    p(&["lautstärke"], Action::VolumeSet),
    p(&["lautstärke", "auf"], Action::VolumeSet),
    p(&["move"], Action::MoveToWorkspace),
    p(&["move", "to"], Action::MoveToWorkspace),
    p(&["move", "to", "tag"], Action::MoveToWorkspace),
    p(&["move", "to", "workspace"], Action::MoveToWorkspace),
    p(&["move", "window", "to"], Action::MoveToWorkspace),
    p(&["move", "window", "to", "tag"], Action::MoveToWorkspace),
    p(&["move", "window", "to", "workspace"], Action::MoveToWorkspace),
    p(&["send", "to"], Action::MoveToWorkspace),
    p(&["send", "to", "tag"], Action::MoveToWorkspace),
    p(&["send", "to", "workspace"], Action::MoveToWorkspace),
    p(&["verschiebe"], Action::MoveToWorkspace),
    p(&["verschiebe", "nach"], Action::MoveToWorkspace),
    p(&["verschiebe", "nach", "tag"], Action::MoveToWorkspace),
    p(&["verschiebe", "nach", "arbeitsfläche"], Action::MoveToWorkspace),
    p(&["verschiebe", "auf"], Action::MoveToWorkspace),
    p(&["verschiebe", "auf", "tag"], Action::MoveToWorkspace),
    p(&["verschiebe", "auf", "arbeitsfläche"], Action::MoveToWorkspace),
    p(&["verschieben", "nach"], Action::MoveToWorkspace),
    p(&["verschieben", "auf"], Action::MoveToWorkspace),
    p(&["tag"], Action::ViewWorkspace),
    p(&["workspace"], Action::ViewWorkspace),
    p(&["arbeitsfläche"], Action::ViewWorkspace),
    p(&["view", "tag"], Action::ViewWorkspace),
    p(&["view", "workspace"], Action::ViewWorkspace),
    p(&["show", "tag"], Action::ViewWorkspace),
    p(&["show", "workspace"], Action::ViewWorkspace),
    p(&["zeige", "tag"], Action::ViewWorkspace),
    p(&["zeige", "arbeitsfläche"], Action::ViewWorkspace),
    p(&["projekt"], Action::Project),
    p(&["project"], Action::Project),
    p(&["claude"], Action::Project),
    p(&["open", "project"], Action::Project),
    p(&["öffne", "projekt"], Action::Project),
    p(&["open"], Action::OpenAny),
    p(&["öffne"], Action::OpenAny),
    p(&["folder"], Action::Folder),
    p(&["open", "folder"], Action::Folder),
    p(&["ordner"], Action::Folder),
    p(&["öffne", "ordner"], Action::Folder),
    p(&["files"], Action::Folder),
    p(&["launch"], Action::App),
    p(&["start"], Action::App),
    p(&["run"], Action::App),
    p(&["starte"], Action::App),
    p(&["app"], Action::App),
    p(&["open", "app"], Action::App),
    p(&["switch", "to"], Action::Switch),
    p(&["go", "to"], Action::Switch),
    p(&["focus"], Action::Switch),
    p(&["wechsle", "zu"], Action::Switch),
    p(&["wechsle", "zum"], Action::Switch),
    p(&["gehe", "zu"], Action::Switch),
    p(&["zeige"], Action::Switch),
    p(&["show"], Action::Switch),
    p(&["close"], Action::Close),
    p(&["close", "window"], Action::Close),
    p(&["schließe"], Action::Close),
    p(&["schließe", "fenster"], Action::Close),
    p(&["diagnose", "unit"], Action::DiagnoseUnit),
    p(&["diagnose", "service"], Action::DiagnoseUnit),
    p(&["diagnose", "dienst"], Action::DiagnoseUnit),
    p(&["diagnose", "failed"], Action::DiagnoseUnit),
    p(&["failed", "unit"], Action::DiagnoseUnit),
    p(&["failed", "service"], Action::DiagnoseUnit),
    p(&["diagnose", "container"], Action::DiagnoseContainer),
    p(&["diagnose", "docker"], Action::DiagnoseContainer),
];

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn keyword_ids_are_unique_per_intent_and_every_keyword_validates() {
        let mut seen = HashSet::new();
        for k in KEYWORDS {
            let intent = (k.intent)();
            assert!(intent.validate().is_ok(), "{}: {:?}", k.id, intent.validate());
            assert!(seen.insert((intent.name(), k.id)), "duplicate id {} for {}", k.id, intent.name());
            assert!(!k.phrases.is_empty() && k.phrases.iter().all(|p| !p.trim().is_empty()), "{}", k.id);
            assert!(!k.display.is_empty(), "{}", k.id);
            assert!((0.0..=2.0).contains(&k.rank), "{}", k.id);
        }
    }

    #[test]
    fn keywords_cover_the_stage_one_intents() {
        let names: HashSet<&str> = KEYWORDS.iter().map(|k| (k.intent)().name()).collect();
        for want in [
            "lock",
            "volume",
            "media",
            "screenshot",
            "show_widget",
            "dictate_settings",
            "logout",
            "reboot",
            "poweroff",
            "suspend",
            "previous_workspace",
            "toggle_window",
            "column_display",
            "close_window",
            "explain_clipboard",
            "diagnose",
        ] {
            assert!(names.contains(want), "no keyword for {want}");
        }
        let widgets: HashSet<&str> = KEYWORDS
            .iter()
            .filter_map(|k| match (k.intent)() {
                Intent::ShowWidget { widget } => Some(widget.name()),
                _ => None,
            })
            .collect();
        assert_eq!(widgets.len(), Widget::NAMES.len(), "every widget has a keyword");
        // jeder Zustand von toggle_window und column_display hat ein Schlüsselwort (das Modell kann jeden wählen; die
        // Zeile des Schlüsselworts trägt dann das fokussierte Fenster als Ziel)
        for (all, of) in [
            (
                WindowToggle::NAMES,
                (|i| match i {
                    Intent::ToggleWindow { what } => Some(what.name()),
                    _ => None,
                }) as fn(Intent) -> Option<&'static str>,
            ),
            (ColumnDisplay::NAMES, |i| match i {
                Intent::ColumnDisplay { display } => Some(display.name()),
                _ => None,
            }),
        ] {
            let covered: HashSet<&str> = KEYWORDS.iter().filter_map(|k| of((k.intent)())).collect();
            assert_eq!(covered.len(), all.len(), "{all:?}");
        }
    }

    #[test]
    fn prefix_words_are_lowercase_and_non_empty() {
        for pr in PREFIXES {
            assert!(!pr.words.is_empty());
            for w in pr.words {
                assert!(!w.is_empty() && !w.contains(char::is_whitespace) && *w == w.to_lowercase(), "{w:?}");
            }
        }
    }
}
