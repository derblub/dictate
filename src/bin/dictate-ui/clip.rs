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

//! Einfügen über die Zwischenablage (`ext_data_control_v1`; PLAN-wayland.md 3.4, Spike S3, Entscheidungen 10 und 11).
//!
//! Ablauf: Zwischenablage mit **allen** Typen sichern (Bytes nur im Speicher, nie geloggt, nie an den Daemon),
//! Text mit dem Hinweis `x-kde-passwordManagerHint: secret` anbieten (Verläufe wie Noctalias lassen ihn aus), eine
//! Rundreise (Abholungen gieriger Verläufe gleich nach dem Anbieten sind dann verteilt), `Ctrl+V` über die eigene
//! virtuelle Tastatur, auf eine Abholung eines Texttyps **nach** dem `Ctrl+V` warten (S3) und danach, bis [`QUIET`]
//! lang keine weitere kam, dann alle gesicherten Typen in ihrer Reihenfolge zurücklegen. `dictate-ui` bedient das
//! Zurückgelegte weiter, bis ein anderes Programm die Zwischenablage übernimmt (endet der Anbieter, ist sie leer; S3).
//!
//! **Nicht einfügen, sondern tippen** ([`Outcome::Fallback`]): wenn die Zwischenablage den Hinweis `secret` trägt
//! (Entscheidung 10: gelesen werden dann nur die Typenliste und der Hinweis, das Passwort nie, der Passwortmanager
//! behält Besitz und Ablauf), wenn der Hinweis nicht lesbar ist, ein Typ nicht in der Frist kommt, alles zusammen
//! größer als [`MAX_SAVED_BYTES`] ist oder das Angebot mehr Typen hat, als gemerkt werden (unvollständig zurücklegen
//! wäre schlimmer), wenn die Auswahl **während des Sicherns wechselt** (ein gerade kopiertes Passwort würde sonst
//! ungeprüft übernommen), wenn der Tastenzustand nicht lesbar ist (`Ctrl+V` nur ohne gehaltene Modifikatoren) oder
//! das Gerät fehlt.
//!
//! **Zurücklegen auf jedem Weg** nach dem Anbieten, auch bei Fehlern. **Abbruch** (Mod+Ctrl+E) vor dem `Ctrl+V` legt
//! sofort zurück; danach wird wie sonst gewartet (ein langsames Ziel holte sonst den alten Inhalt). War die
//! Zwischenablage vorher leer, ist sie danach wieder leer. Übernimmt ein anderes Programm die Zwischenablage, bevor
//! zurückgelegt ist (der User kopiert etwas), bleibt dessen Inhalt.

use crate::actions::selection::{MAX_BYTES, TIMEOUT};
use crate::actions::store::SecretText;
use crate::config::ACTIONS_MAX_CHARS;
use crate::primary::{decode, TEXT_TYPES as PRIMARY_TYPES};
use crate::ui::panel::CLIPBOARD_PASSWORD;
use crate::wl::{set_nonblocking, Items, Secret, Vkbd, Wl, HINT};
use std::io::ErrorKind;
use std::os::fd::{AsFd, AsRawFd, FromRawFd, OwnedFd};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{debug, info};
use wayland_client::Proxy;
use wayland_protocols::ext::data_control::v1::client::{ext_data_control_offer_v1::ExtDataControlOfferV1, ext_data_control_source_v1::ExtDataControlSourceV1};

/// Gesicherte Zwischenablage höchstens so groß (alle Typen zusammen, wie `paste.rs` unter X)
pub const MAX_SAVED_BYTES: usize = 32 * 1024 * 1024;
/// Frist für das Sichern aller Typen zusammen
const SAVE_TIMEOUT: Duration = Duration::from_secs(2);
/// Frist für das Lesen des Hinweises
const HINT_TIMEOUT: Duration = Duration::from_millis(500);
/// Ruhe nach der letzten Abholung (Programme holen oft mehrere Typen nacheinander)
pub const QUIET: Duration = Duration::from_millis(300);
/// Frist bis zur Abholung nach dem `Ctrl+V` (auch die Höchstdauer des Wartens auf Ruhe)
pub const CONFIRM_TIMEOUT: Duration = Duration::from_secs(5);
/// Typen, unter denen der Text angeboten wird
const TEXT_TYPES: [&str; 4] = ["text/plain;charset=utf-8", "text/plain", "UTF8_STRING", "TEXT"];

/// Ergebnis eines Einfügens
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Abgeholt und zurückgelegt
    Pasted,
    /// In der Frist nicht abgeholt (trotzdem zurückgelegt)
    Unconfirmed,
    /// Abgebrochen (zurückgelegt)
    Stopped,
    /// Nicht eingefügt, stattdessen tippen (Grund fürs Log, ohne Inhalt)
    Fallback(&'static str),
    /// Nicht eingefügt und auch nicht tippen (Grund für den Hinweis; zurückgelegt)
    Failed(String),
}

/// Ergebnis des `Ctrl+V`-Schritts
pub enum CtrlV {
    Sent,
    /// Tastenzustand nicht lesbar: nicht einfügen, tippen
    Unreadable,
    Stopped,
    /// Frist der Modifikatoren, Fokuswechsel oder Fehler: aufgeben
    Failed(String),
}

/// Die gesicherte Zwischenablage: `None` = war leer
type Saved = Option<Vec<(String, Arc<Secret>)>>;

/// Prüft und sichert die Zwischenablage; liefert sie und die Nummer der Auswahl. `Err(grund)`: nicht einfügen (tippen)
fn save(wl: &mut Wl) -> Result<(Saved, u64), &'static str> {
    // Ausstehende Ereignisse zuerst (eine gerade übernommene Auswahl, `cancelled` der eigenen)
    wl.roundtrip().map_err(|_| "wayland error")?;
    if !wl.st.selection_known {
        return Err("clipboard state unknown");
    }
    let seq = wl.st.selection_seq;
    // Gehört die Zwischenablage noch der eigenen zurückgelegten Quelle: die Bytes liegen schon im Speicher (aus der
    // eigenen Quelle zu lesen, während dieser Thread sie bedient, hinge bis zur Frist)
    if let Some((_, items)) = &wl.st.restored {
        debug!("clipboard: still our restored content ({} types)", items.len());
        return Ok((Some(items.iter().cloned().collect()), seq));
    }
    let Some(sel) = wl.st.selection.clone() else { return Ok((None, seq)) };
    if sel.truncated {
        return Err("clipboard offers too many types");
    }
    if sel.types.iter().any(|t| t == HINT) {
        let v = read_offer(wl, &sel.offer, HINT, 64, Instant::now() + HINT_TIMEOUT).map_err(|_| "password hint unreadable")?;
        if wl.st.selection_seq != seq {
            return Err("clipboard changed while saving");
        }
        if v.0.trim_ascii() == b"secret" {
            return Err("clipboard holds a password");
        }
    }
    let deadline = Instant::now() + SAVE_TIMEOUT;
    let mut total = 0usize;
    let mut items = Vec::with_capacity(sel.types.len());
    for t in &sel.types {
        let d = read_offer(wl, &sel.offer, t, MAX_SAVED_BYTES - total, deadline)?;
        // Ein Wechsel mitten im Sichern (das alte Angebot ist dann zerstört, sein Lesen endet leer): nie übernehmen
        if wl.st.selection_seq != seq {
            return Err("clipboard changed while saving");
        }
        total += d.0.len();
        items.push((t.clone(), Arc::new(d)));
    }
    debug!("clipboard: saved {} types, {total} bytes", items.len());
    Ok((Some(items), seq))
}

/// `buf` um `extra` Bytes vergrößern; beim Umziehen wird der alte Speicher überschrieben (keine Kopien des Inhalts im
/// freigegebenen Speicher)
fn grow_wiping(buf: &mut Vec<u8>, extra: usize) {
    if buf.capacity() - buf.len() >= extra {
        return;
    }
    let want = (buf.len() + extra).max(buf.capacity() * 2).max(64 * 1024);
    let mut new = Vec::with_capacity(want);
    new.extend_from_slice(buf);
    crate::actions::store::wipe_bytes(buf);
    *buf = new;
}

/// CLIPBOARD als Text lesen, **nur** für die Absicht `explain_clipboard` der Befehlspalette (Lesart L8): einziger
/// Aufrufer ist `Cmd::ReadClipboard` im Ausgabe-Thread, den nur `act.rs` für die Quelle `clipboard` schickt (Test
/// `sources_stay_apart`). Grenzen wie bei PRIMARY (`actions::selection`: [`MAX_BYTES`], Frist [`TIMEOUT`]), Kodierung
/// wie dort. Trägt die Zwischenablage den Hinweis `x-kde-passwordManagerHint: secret`, wird nur er gelesen und
/// abgelehnt (Entscheidung 10). Gehört sie noch einer eigenen Quelle (zurückgelegt oder Copy), kommt der Text aus dem
/// Speicher (aus der eigenen Quelle zu lesen, während dieser Thread sie bedient, hinge bis zur Frist). Ohne Inhalt oder
/// ohne Texttyp ist das Ergebnis leer (der Daemon antwortet „The clipboard is empty“). Inhalt nie loggen, nur Längen;
/// gesendet wird er erst nach der Vorschau (`act_run`).
pub fn read_clipboard_text(wl: &mut Wl) -> Result<SecretText, String> {
    wl.roundtrip().map_err(|e| format!("Cannot read the clipboard: {e}"))?;
    if wl.device.is_none() {
        return Err("Cannot read the clipboard: the compositor offers no data control".into());
    }
    if !wl.st.selection_known {
        return Err("Cannot read the clipboard: the compositor reports no clipboard".into());
    }
    let too_long = || format!("Text too long: more than {ACTIONS_MAX_CHARS} characters");
    let not_utf8 = |_| "The clipboard is not valid UTF-8 text".to_string();
    if let Some((_, items)) = &wl.st.restored {
        if items.iter().any(|(t, v)| t == HINT && v.0.trim_ascii() == b"secret") {
            return Err(CLIPBOARD_PASSWORD.into());
        }
        let Some((mime, latin1, data)) = PRIMARY_TYPES.iter().find_map(|(m, l)| items.iter().find(|(t, _)| t == m).map(|(_, d)| (*m, *l, d.clone()))) else {
            debug!("clipboard: our own content has no text type");
            return Ok(SecretText::new(String::new()));
        };
        if data.0.len() > MAX_BYTES {
            return Err(too_long());
        }
        let text = decode(data.0.clone(), latin1).map_err(not_utf8)?;
        debug!("clipboard: {} bytes as {mime} from our own source", text.as_str().len());
        return Ok(text);
    }
    let seq = wl.st.selection_seq;
    let Some(sel) = wl.st.selection.clone() else {
        debug!("clipboard: empty");
        return Ok(SecretText::new(String::new()));
    };
    let deadline = Instant::now() + TIMEOUT;
    // der Hinweis wird immer gemerkt, auch bei abgeschnittener Typenliste (`wl::Offer::truncated`)
    if sel.types.iter().any(|t| t == HINT) {
        let v = read_offer(wl, &sel.offer, HINT, 64, deadline).map_err(|_| "Cannot read the clipboard: its password hint is unreadable".to_string())?;
        if v.0.trim_ascii() == b"secret" {
            return Err(CLIPBOARD_PASSWORD.into());
        }
    }
    let Some((mime, latin1)) = PRIMARY_TYPES.iter().copied().find(|(m, _)| sel.types.iter().any(|t| t == m)) else {
        debug!("clipboard: no text type among {} types", sel.types.len());
        return Ok(SecretText::new(String::new()));
    };
    let mut data = read_offer(wl, &sel.offer, mime, MAX_BYTES, deadline).map_err(|e| match e {
        "clipboard too large" => too_long(),
        "clipboard owner too slow" => format!("Cannot read the clipboard: no answer within {} s", TIMEOUT.as_secs()),
        other => format!("Cannot read the clipboard: {}", other.trim_start_matches("clipboard ")),
    })?;
    // Wechsel mitten im Lesen (das alte Angebot ist dann zerstört): nie einen halben Inhalt übergeben
    if wl.st.selection_seq != seq {
        return Err("The clipboard changed while reading it. Try again.".into());
    }
    let text = decode(std::mem::take(&mut data.0), latin1).map_err(not_utf8)?;
    debug!("clipboard: {} bytes as {mime}", text.as_str().len());
    Ok(text)
}

/// Einen Typ eines Angebots lesen, höchstens `limit` Bytes bis `deadline`, und dabei weiter verteilen. Einzige Stelle,
/// die ein Angebot empfängt; aufgerufen nur für CLIPBOARD (hier: Sichern beim Einfügen und [`read_clipboard_text`])
/// und PRIMARY (`primary.rs`), der Test `sources_stay_apart` in `primary.rs` hält das fest
pub fn read_offer(wl: &mut Wl, offer: &ExtDataControlOfferV1, mime: &str, limit: usize, deadline: Instant) -> Result<Secret, &'static str> {
    let mut fds = [0; 2];
    // SAFETY: Feld mit zwei Plätzen
    if unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) } != 0 {
        return Err("pipe failed");
    }
    // SAFETY: frisch angelegt, gehören niemandem sonst
    let (r, w) = unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) };
    set_nonblocking(r.as_raw_fd());
    offer.receive(mime.to_string(), w.as_fd());
    if wl.flush().is_err() {
        return Err("wayland error");
    }
    drop(w);
    let mut buf = Secret(Vec::new());
    let mut chunk = Secret(vec![0u8; 64 * 1024]);
    loop {
        // SAFETY: gültiger Deskriptor und Puffer
        let n = unsafe { libc::read(r.as_raw_fd(), chunk.0.as_mut_ptr().cast(), chunk.0.len()) };
        if n == 0 {
            return Ok(buf);
        }
        if n > 0 {
            let n = n as usize;
            if buf.0.len() + n > limit {
                return Err("clipboard too large");
            }
            grow_wiping(&mut buf.0, n);
            buf.0.extend_from_slice(&chunk.0[..n]);
            continue;
        }
        let e = std::io::Error::last_os_error();
        if e.kind() == ErrorKind::Interrupted {
            continue;
        }
        if e.kind() != ErrorKind::WouldBlock {
            return Err("clipboard read failed");
        }
        if Instant::now() >= deadline {
            return Err("clipboard owner too slow");
        }
        if wl.pump_with(deadline, Some(r.as_raw_fd())).is_err() {
            return Err("wayland error");
        }
    }
}

/// Eigene Quelle mit `items` als Auswahl setzen
fn offer(wl: &mut Wl, items: Items) -> Option<ExtDataControlSourceV1> {
    let (mgr, dev) = (wl.dc_mgr.as_ref()?, wl.device.as_ref()?);
    let src = mgr.create_data_source(&wl.qh, items.clone());
    for (t, _) in items.iter() {
        src.offer(t.clone());
    }
    dev.set_selection(Some(&src));
    Some(src)
}

/// Gesichertes zurücklegen (oder leeren, wenn sie leer war); `dictate-ui` bedient es danach weiter
fn restore(wl: &mut Wl, saved: Saved, ours: &ExtDataControlSourceV1) {
    let _ = wl.pump(Instant::now());
    if wl.st.cancelled.contains(&ours.id()) {
        // Ein anderes Programm hat die Zwischenablage inzwischen übernommen: dessen Inhalt bleibt
        info!("clipboard: taken over by another program before restoring, left as is");
        return;
    }
    match saved {
        None => {
            if let Some(dev) = &wl.device {
                dev.set_selection(None);
            }
            debug!("clipboard: cleared again (was empty)");
        }
        Some(items) => {
            let n = items.len();
            let items: Items = Arc::new(items);
            if let Some(src) = offer(wl, items.clone()) {
                // Die vorige eigene Quelle (falls noch da) wird von `cancelled` aufgeräumt
                wl.st.restored = Some((src, items));
                debug!("clipboard: restored {n} types");
            }
        }
    }
    let _ = wl.flush();
}

/// Copy der Textaktionen: `text` als neue Zwischenablage anbieten, ohne Hinweis (Entscheidung 12: bewusstes Kopieren
/// darf in den Verlauf). `dictate-ui` bedient sie weiter, bis ein anderes Programm sie übernimmt; ein späteres Einfügen
/// sichert sie aus dem Speicher (wie Zurückgelegtes, `save`)
pub fn copy(wl: &mut Wl, text: SecretText) -> Result<(), String> {
    let data = Arc::new(Secret(text.as_str().as_bytes().to_vec()));
    drop(text);
    let items: Items = Arc::new(TEXT_TYPES.iter().map(|t| (t.to_string(), data.clone())).collect());
    // Objekt-IDs zerstörter Quellen werden wiederverwendet: alte Meldungen dürfen nicht zur neuen Quelle passen
    wl.st.cancelled.clear();
    wl.st.sends.clear();
    let src = offer(wl, items.clone()).ok_or("the compositor offers no data control")?;
    // Die vorige eigene Quelle bekommt `cancelled` und wird dort freigegeben
    wl.st.restored = Some((src, items));
    wl.flush()?;
    debug!("clipboard: copied text offered as {} types", TEXT_TYPES.len());
    Ok(())
}

/// Text einfügen. `stopped()`: Mod+Ctrl+E kam; `ctrl_v()` wartet auf losgelassene Modifikatoren, prüft den Fokus und
/// sendet das `Ctrl+V` (oder sagt, warum nicht)
pub fn paste(wl: &mut Wl, text: &str, kbd: &mut Vkbd, stopped: &dyn Fn() -> bool, ctrl_v: &mut dyn FnMut(&mut Wl, &mut Vkbd) -> CtrlV) -> Outcome {
    if wl.device.is_none() {
        return Outcome::Fallback("no data control");
    }
    let (saved, seq) = match save(wl) {
        Ok(s) => s,
        Err(why) => return Outcome::Fallback(why),
    };
    if stopped() {
        return Outcome::Stopped;
    }
    // Letzte Prüfung vor der Übernahme: dieselbe Auswahl wie beim Sichern
    if let Err(e) = wl.roundtrip() {
        return Outcome::Failed(e);
    }
    if wl.st.selection_seq != seq {
        return Outcome::Fallback("clipboard changed while saving");
    }
    let text = Arc::new(Secret(text.as_bytes().to_vec()));
    let mut items: Vec<(String, Arc<Secret>)> = TEXT_TYPES.iter().map(|t| (t.to_string(), text.clone())).collect();
    items.push((HINT.into(), Arc::new(Secret(b"secret".to_vec()))));
    // Objekt-IDs zerstörter Quellen werden wiederverwendet: alte Meldungen dürfen nicht zur neuen Quelle passen
    wl.st.cancelled.clear();
    wl.st.sends.clear();
    let Some(ours) = offer(wl, Arc::new(items)) else { return Outcome::Fallback("no data control") };
    let out = deliver(wl, kbd, &ours, stopped, ctrl_v);
    // Zurücklegen auf jedem Weg (auch nach einem Fehler: sonst bliebe der Text samt Hinweis in der Zwischenablage)
    restore(wl, saved, &ours);
    out
}

/// Nach dem Anbieten: `Ctrl+V` und auf die Abholung warten
fn deliver(
    wl: &mut Wl,
    kbd: &mut Vkbd,
    ours: &ExtDataControlSourceV1,
    stopped: &dyn Fn() -> bool,
    ctrl_v: &mut dyn FnMut(&mut Wl, &mut Vkbd) -> CtrlV,
) -> Outcome {
    // Abholungen gieriger Verläufe gleich nach dem Anbieten verteilen, bevor die Zeit des Ctrl+V zählt
    if let Err(e) = wl.roundtrip() {
        return Outcome::Failed(e);
    }
    match ctrl_v(wl, kbd) {
        CtrlV::Sent => {}
        CtrlV::Unreadable => return Outcome::Fallback("modifier state unknown"),
        CtrlV::Stopped => return Outcome::Stopped,
        CtrlV::Failed(e) => return Outcome::Failed(e),
    }
    let sent_at = Instant::now();
    let deadline = sent_at + CONFIRM_TIMEOUT;
    let mut last: Option<Instant> = None;
    loop {
        let now = Instant::now();
        if now >= deadline || last.is_some_and(|l| now >= l + QUIET) || wl.st.cancelled.contains(&ours.id()) {
            break;
        }
        let until = last.map_or(deadline, |l| (l + QUIET).min(deadline)).min(now + Duration::from_millis(20));
        if let Err(e) = wl.pump(until) {
            return Outcome::Failed(e);
        }
        // Ruhe zählt ab der letzten Abholung eines Texttyps nach dem Ctrl+V
        last = wl.st.sends.iter().filter(|s| s.source == ours.id() && s.at >= sent_at && TEXT_TYPES.contains(&s.mime.as_str())).map(|s| s.at).max();
    }
    if stopped() {
        Outcome::Stopped
    } else if last.is_some() {
        Outcome::Pasted
    } else {
        Outcome::Unconfirmed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grow_keeps_content_and_reserves_ahead() {
        let mut b = Vec::new();
        grow_wiping(&mut b, 10);
        assert!(b.capacity() >= 64 * 1024);
        b.extend_from_slice(b"abc");
        let cap = b.capacity();
        grow_wiping(&mut b, 10);
        assert_eq!(b.capacity(), cap, "enough room: no move");
        grow_wiping(&mut b, cap);
        assert!(b.capacity() >= 3 + cap);
        assert_eq!(b, b"abc");
    }
}
