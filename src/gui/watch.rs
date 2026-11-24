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

//! Beobachter einer Datei für das Einstellungsfenster (Palette von Noctalia; Lesart L43 und Regel 4 in
//! PLAN-wayland.md 6h): inotify über `libc` auf den **Ordner** der Datei (fängt Ersetzen per `rename`, wie Noctalia
//! schreibt, Neuanlage und Löschen), in einem eigenen Thread; Ereignisse zählen nur für den Dateinamen (im selben
//! Ordner schreibt der Daemon `frecency.json` und `diagnose.json`, im Test liegen dort die Logs). Nach 50 ms ohne
//! weiteres Ereignis (Entprellung wie `dictate-ui`) merkt der Thread den Zeitpunkt des ersten Ereignisses und weckt
//! das Fenster (`wake`, im Fenster `ctx.request_repaint()`); das Fenster liest die Datei dann selbst und färbt nur um,
//! wenn sich die Palette unterscheidet.
//!
//! Fehlt der Ordner, beobachtet der Thread den nächsten vorhandenen Vorfahren, bis der Ordner entsteht; verschwindet
//! er, beginnt das von vorn. Nach jedem Scharfstellen weckt er einmal (die Datei kann sich vorher geändert haben,
//! etwa zwischen dem Laden beim Start und dem ersten Scharfstellen). Fällt inotify aus, prüft der Thread alle 2 s
//! Änderungszeit, Größe und Inode (Grund im Log) und weckt nur bei einer Änderung: kein Neuzeichnen im Leerlauf
//! (Messpunkt M3; Abweichung vom Wortlaut von L43, das `request_repaint_after` im Fenster nannte).
//!
//! Folgt keinem Symlink auf ein anderes Ziel: beobachtet wird der Eintrag im Ordner.

use std::ffi::{CString, OsStr};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Ruhe nach dem letzten Ereignis, bevor das Fenster geweckt wird
pub const DEBOUNCE: Duration = Duration::from_millis(50);
/// Abstand der Prüfungen im Rückfall ohne inotify
pub const POLL: Duration = Duration::from_secs(2);

type Wake = Box<dyn Fn() + Send>;

/// Gemeinsamer Zustand: Zeitpunkt des ersten Ereignisses seit dem letzten [`Watch::take`]
pub struct Watch {
    pending: Arc<Mutex<Option<Instant>>>,
}

impl Watch {
    /// Startet den Thread für `path`; Gründe für Rückfälle schreibt er mit dem Präfix `log` nach stderr
    pub fn spawn(path: &Path, log: &'static str, wake: impl Fn() + Send + 'static) -> Watch {
        let pending = Arc::new(Mutex::new(None));
        let w = Watcher { path: path.to_path_buf(), log, pending: Arc::clone(&pending), wake: Box::new(wake) };
        std::thread::Builder::new().name("palette-watch".into()).spawn(move || w.run()).expect("thread");
        Watch { pending }
    }

    /// Zeitpunkt des ersten Ereignisses seit dem letzten Abholen (nach der Entprellung), sonst `None`
    pub fn take(&self) -> Option<Instant> {
        self.pending.lock().unwrap_or_else(|e| e.into_inner()).take()
    }
}

struct Watcher {
    path: PathBuf,
    log: &'static str,
    pending: Arc<Mutex<Option<Instant>>>,
    wake: Wake,
}

impl Watcher {
    fn signal(&self, first: Instant) {
        {
            let mut p = self.pending.lock().unwrap_or_else(|e| e.into_inner());
            if p.is_none() {
                *p = Some(first);
            }
        }
        (self.wake)();
    }

    fn run(self) {
        let (Some(dir), Some(name)) = (self.path.parent().filter(|d| !d.as_os_str().is_empty()), self.path.file_name()) else {
            eprintln!("{}: palette watch: {} has no folder, checking every {} s", self.log, self.path.display(), POLL.as_secs());
            return self.poll();
        };
        let why = match Inotify::new() {
            Ok(ino) => match self.inotify(&ino, dir, name) {
                Ok(never) => match never {},
                Err(e) => e,
            },
            Err(e) => e,
        };
        eprintln!("{}: palette watch: inotify unavailable ({why}), checking every {} s", self.log, POLL.as_secs());
        self.poll()
    }

    fn inotify(&self, ino: &Inotify, dir: &Path, name: &OsStr) -> Result<std::convert::Infallible, String> {
        loop {
            // Scharfstellen: der Ordner selbst, sonst der nächste vorhandene Vorfahr (wartet auf die Neuanlage des
            // Eintrags `step` auf dem Weg zum Ordner; andere Einträge dort wecken nicht)
            let (wd, on_dir, step) = if dir.is_dir() {
                match ino.add(dir, DIR_MASK) {
                    Ok(wd) => (wd, true, None),
                    // verschwand zwischen Prüfen und Scharfstellen: neu prüfen statt für immer abzufragen
                    Err(_) if !dir.is_dir() => continue,
                    Err(e) => return Err(e),
                }
            } else {
                let mut a = dir;
                while !a.is_dir() {
                    a = a.parent().ok_or_else(|| format!("no existing folder above {}", dir.display()))?;
                }
                let step = dir.strip_prefix(a).ok().and_then(|r| r.components().next()).map(|c| c.as_os_str().as_bytes().to_vec());
                let wd = match ino.add(a, ANCESTOR_MASK) {
                    Ok(wd) => wd,
                    Err(_) if !a.is_dir() => continue,
                    Err(e) => return Err(e),
                };
                if dir.is_dir() {
                    // entstand zwischen Prüfen und Scharfstellen: gleich auf den Ordner
                    ino.remove(wd);
                    continue;
                }
                (wd, false, step)
            };
            self.signal(Instant::now());
            // Ereignisse bis zum nächsten Neu-Scharfstellen
            // erstes und letztes passendes Ereignis seit dem letzten Wecken; die Ruhezeit zählt ab dem letzten
            // passenden, Ereignisse anderer Dateien im Ordner verlängern sie nicht
            let mut first: Option<Instant> = None;
            let mut last = Instant::now();
            let rearm = loop {
                let timeout = first.map(|_| DEBOUNCE.saturating_sub(last.elapsed()));
                if timeout == Some(Duration::ZERO) {
                    if let Some(t) = first.take() {
                        self.signal(t);
                    }
                    continue;
                }
                let events = ino.read(timeout)?;
                let mut rearm = false;
                let mut hit = false;
                for ev in &events {
                    if ev.mask & libc::IN_Q_OVERFLOW != 0 {
                        // Ereignisse verloren, womöglich auch das Ende des Watches: neu lesen und neu scharfstellen
                        hit = true;
                        rearm = true;
                    } else if ev.wd != wd {
                        continue;
                    } else if ev.mask & (libc::IN_DELETE_SELF | libc::IN_MOVE_SELF | libc::IN_IGNORED) != 0 {
                        rearm = true;
                    } else if !on_dir {
                        // auf dem Vorfahren: nur der Eintrag auf dem Weg zum Ordner zählt
                        if ev.name.is_some() && ev.name.as_deref() == step.as_deref() {
                            rearm = true;
                        }
                    } else if ev.name.as_deref() == Some(name.as_bytes()) {
                        hit = true;
                    }
                }
                if hit {
                    last = Instant::now();
                    first.get_or_insert(last);
                }
                if rearm {
                    break true;
                }
            };
            if rearm {
                ino.remove(wd);
                if let Some(t) = first.take() {
                    self.signal(t);
                }
            }
        }
    }

    fn poll(&self) {
        let stamp = |p: &Path| std::fs::metadata(p).ok().map(|m| (m.dev(), m.ino(), m.len(), m.mtime(), m.mtime_nsec()));
        let mut last = stamp(&self.path);
        loop {
            std::thread::sleep(POLL);
            let now = stamp(&self.path);
            if now != last {
                last = now;
                self.signal(Instant::now());
            }
        }
    }
}

/// Ordner der Datei: Datei geschrieben, umbenannt, neu, gelöscht; der Ordner selbst verschwindet
const DIR_MASK: u32 = libc::IN_CLOSE_WRITE
    | libc::IN_MOVED_TO
    | libc::IN_MOVED_FROM
    | libc::IN_CREATE
    | libc::IN_DELETE
    | libc::IN_DELETE_SELF
    | libc::IN_MOVE_SELF
    | libc::IN_ONLYDIR;
/// Vorfahr, solange der Ordner fehlt: ein Eintrag entsteht oder der Vorfahr verschwindet
const ANCESTOR_MASK: u32 = libc::IN_CREATE | libc::IN_MOVED_TO | libc::IN_DELETE_SELF | libc::IN_MOVE_SELF | libc::IN_ONLYDIR;

/// Frist für `poll` in ms, aufgerundet: abgeschnitten käme `poll(0)` vor dem Ende der Entprellung zurück, und die
/// Schleife drehte bis zu 1 ms leer
fn poll_ms(t: Duration) -> i32 {
    i32::try_from(t.as_micros().div_ceil(1000)).unwrap_or(i32::MAX)
}

struct Event {
    wd: i32,
    mask: u32,
    name: Option<Vec<u8>>,
}

/// Dünne Hülle um einen inotify-Deskriptor (nicht blockierend, gelesen nach `poll`)
struct Inotify {
    fd: std::os::fd::OwnedFd,
}

impl Inotify {
    fn new() -> Result<Inotify, String> {
        use std::os::fd::FromRawFd;
        // SAFETY: einfacher Systemaufruf; Ergebnis ist ein neuer Deskriptor oder -1
        let fd = unsafe { libc::inotify_init1(libc::IN_NONBLOCK | libc::IN_CLOEXEC) };
        if fd < 0 {
            return Err(format!("inotify_init1: {}", std::io::Error::last_os_error()));
        }
        // SAFETY: frisch geöffnet, gehört niemandem sonst
        Ok(Inotify { fd: unsafe { std::os::fd::OwnedFd::from_raw_fd(fd) } })
    }

    fn raw(&self) -> i32 {
        use std::os::fd::AsRawFd;
        self.fd.as_raw_fd()
    }

    fn add(&self, dir: &Path, mask: u32) -> Result<i32, String> {
        let c = CString::new(dir.as_os_str().as_bytes()).map_err(|_| format!("{}: bad path", dir.display()))?;
        // SAFETY: gültiger Deskriptor und nullterminierter Pfad
        let wd = unsafe { libc::inotify_add_watch(self.raw(), c.as_ptr(), mask) };
        if wd < 0 {
            return Err(format!("inotify_add_watch {}: {}", dir.display(), std::io::Error::last_os_error()));
        }
        Ok(wd)
    }

    fn remove(&self, wd: i32) {
        // SAFETY: gültiger Deskriptor; ein schon entfernter Watch liefert nur EINVAL
        unsafe { libc::inotify_rm_watch(self.raw(), wd) };
    }

    /// Wartet höchstens `timeout` (ohne: unbegrenzt) und liest alle anstehenden Ereignisse; leer = Frist abgelaufen
    fn read(&self, timeout: Option<Duration>) -> Result<Vec<Event>, String> {
        let ms = timeout.map_or(-1, poll_ms);
        let mut pfd = libc::pollfd { fd: self.raw(), events: libc::POLLIN, revents: 0 };
        // SAFETY: ein gültiges pollfd
        let n = unsafe { libc::poll(&mut pfd, 1, ms) };
        if n < 0 {
            let e = std::io::Error::last_os_error();
            return if e.kind() == std::io::ErrorKind::Interrupted { Ok(Vec::new()) } else { Err(format!("poll: {e}")) };
        }
        if n == 0 {
            return Ok(Vec::new());
        }
        // ausgerichtet wie `struct inotify_event`
        let mut buf = [0u64; 1024];
        let mut out = Vec::new();
        loop {
            // SAFETY: Puffer gültig und groß genug für mindestens ein Ereignis mit Namen (NAME_MAX + 1)
            let len = unsafe { libc::read(self.raw(), buf.as_mut_ptr().cast(), std::mem::size_of_val(&buf)) };
            if len < 0 {
                let e = std::io::Error::last_os_error();
                if e.kind() == std::io::ErrorKind::WouldBlock || e.kind() == std::io::ErrorKind::Interrupted {
                    break;
                }
                return Err(format!("read: {e}"));
            }
            if len == 0 {
                break;
            }
            // SAFETY: `len` Bytes wurden in den Puffer geschrieben
            let bytes = unsafe { std::slice::from_raw_parts(buf.as_ptr().cast::<u8>(), len as usize) };
            out.extend(parse_events(bytes));
        }
        Ok(out)
    }
}

/// Ereignisse aus dem Puffer von `read` (Kopf: wd i32, mask u32, cookie u32, len u32; danach `len` Bytes Name mit
/// Nullen aufgefüllt)
fn parse_events(mut bytes: &[u8]) -> Vec<Event> {
    const HEAD: usize = 16;
    let mut out = Vec::new();
    while bytes.len() >= HEAD {
        let word = |i: usize| u32::from_ne_bytes([bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]]);
        let (wd, mask, len) = (word(0) as i32, word(4), word(12) as usize);
        let Some(raw) = bytes.get(HEAD..HEAD + len) else { break };
        let name: Vec<u8> = raw.iter().copied().take_while(|b| *b != 0).collect();
        out.push(Event { wd, mask, name: (!name.is_empty()).then_some(name) });
        bytes = &bytes[HEAD + len..];
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    struct Dir(PathBuf);
    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn tmp(tag: &str) -> Dir {
        let d = std::env::temp_dir().join(format!("dictate-watch-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        Dir(d)
    }

    /// Startet den Beobachter, wartet das Wecken nach dem Scharfstellen ab
    fn watch(path: &Path) -> (Watch, mpsc::Receiver<()>) {
        let (tx, rx) = mpsc::channel();
        let w = Watch::spawn(path, "test", move || {
            let _ = tx.send(());
        });
        rx.recv_timeout(Duration::from_secs(2)).expect("wakes after arming");
        assert!(w.take().is_some());
        (w, rx)
    }

    fn woke(rx: &mpsc::Receiver<()>) -> bool {
        rx.recv_timeout(Duration::from_secs(2)).is_ok()
    }

    fn quiet(rx: &mpsc::Receiver<()>) -> bool {
        rx.recv_timeout(Duration::from_millis(300)).is_err()
    }

    /// Weckrufe abwarten, bis 300 ms Ruhe ist (Scharfstellen über mehrere Stufen weckt je Stufe)
    fn settle(rx: &mpsc::Receiver<()>) {
        while !quiet(rx) {}
    }

    #[test]
    fn rename_wakes_other_names_do_not() {
        let d = tmp("rename");
        let p = d.0.join("palette.json");
        std::fs::write(&p, "a").unwrap();
        let (w, rx) = watch(&p);
        // derselbe Ordner, andere Dateien (Daemon, Logs): kein Wecken
        std::fs::write(d.0.join("frecency.json"), "x").unwrap();
        std::fs::write(d.0.join("palette.json.tmp"), "b").unwrap();
        assert!(quiet(&rx));
        // Ersetzen per rename wie Noctalia
        std::fs::rename(d.0.join("palette.json.tmp"), &p).unwrap();
        assert!(woke(&rx));
        assert!(w.take().is_some());
        assert!(w.take().is_none());
        // Löschen
        std::fs::remove_file(&p).unwrap();
        assert!(woke(&rx));
    }

    /// Mehrere Schreibvorgänge kurz hintereinander: einmal wecken, Zeitpunkt des ersten Ereignisses
    #[test]
    fn bursts_are_debounced() {
        let d = tmp("burst");
        let p = d.0.join("palette.json");
        let (w, rx) = watch(&p);
        let before = Instant::now();
        std::fs::write(&p, "0").unwrap();
        for i in 1..5 {
            std::thread::sleep(Duration::from_millis(10));
            std::fs::write(&p, format!("{i}")).unwrap();
        }
        assert!(woke(&rx));
        let woken = Instant::now();
        let first = w.take().unwrap();
        // Zeitpunkt des ersten gelesenen Ereignisses, nicht der des Weckens: geweckt wird frühestens 50 ms nach dem
        // letzten passenden Ereignis (unabhängig davon, wie spät der Thread unter Last liest). Ein zweites Wecken
        // unter Last ist erlaubt.
        assert!(first >= before && first + DEBOUNCE <= woken, "Zeitpunkt des ersten Ereignisses");
        settle(&rx);
    }

    /// Ereignisse anderer Dateien im Ordner (Logs, Daemon) verlängern die Ruhezeit nicht
    #[test]
    fn other_files_do_not_starve_the_debounce() {
        let d = tmp("starve");
        let p = d.0.join("palette.json");
        let (_w, rx) = watch(&p);
        let other = d.0.join("frecency.json");
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let s2 = Arc::clone(&stop);
        let o2 = other.clone();
        let noise = std::thread::spawn(move || {
            let mut i = 0u32;
            while !s2.load(std::sync::atomic::Ordering::Relaxed) {
                std::fs::write(&o2, i.to_string()).unwrap();
                i += 1;
                std::thread::sleep(Duration::from_millis(10));
            }
        });
        std::thread::sleep(Duration::from_millis(50));
        std::fs::write(&p, "x").unwrap();
        let woke_within = rx.recv_timeout(Duration::from_millis(1000)).is_ok();
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        noise.join().unwrap();
        assert!(woke_within, "trotz Schreibvorgängen an anderer Datei alle 10 ms");
    }

    /// Ordner fehlt beim Start, entsteht später, verschwindet und kommt wieder
    #[test]
    fn missing_folder_is_followed() {
        let d = tmp("missing");
        let dir = d.0.join("a/b");
        let p = dir.join("palette.json");
        let (_w, rx) = watch(&p);
        std::fs::create_dir_all(&dir).unwrap();
        assert!(woke(&rx), "neu scharf auf dem Ordner");
        settle(&rx);
        std::fs::write(&p, "x").unwrap();
        assert!(woke(&rx));
        settle(&rx);
        std::fs::remove_dir_all(d.0.join("a")).unwrap();
        assert!(woke(&rx), "Ordner weg");
        settle(&rx);
        std::fs::create_dir_all(&dir).unwrap();
        assert!(woke(&rx), "Ordner wieder da");
        settle(&rx);
        std::fs::write(&p, "y").unwrap();
        assert!(woke(&rx));
    }

    /// Befund der Durchsicht in Block E: fehlt der Ordner, weckt nur der Eintrag auf dem Weg zu ihm, nicht jede
    /// Neuanlage im Vorfahren (etwa andere Programme unter `~/.local/state`)
    #[test]
    fn missing_folder_ignores_unrelated_entries() {
        let d = tmp("unrelated");
        let dir = d.0.join("dictate");
        let p = dir.join("palette.json");
        let (_w, rx) = watch(&p);
        std::fs::create_dir(d.0.join("other-tool")).unwrap();
        std::fs::write(d.0.join("other-file"), "x").unwrap();
        assert!(quiet(&rx), "fremde Einträge im Vorfahren wecken nicht");
        std::fs::create_dir(&dir).unwrap();
        assert!(woke(&rx), "der Ordner selbst weckt");
        settle(&rx);
        std::fs::write(&p, "x").unwrap();
        assert!(woke(&rx));
    }

    /// Befund der Durchsicht in Block E: die Frist wird aufgerundet, sonst käme `poll(0)` vor dem Ende der Entprellung
    #[test]
    fn poll_timeout_rounds_up() {
        assert_eq!(poll_ms(Duration::ZERO), 0);
        assert_eq!(poll_ms(Duration::from_micros(400)), 1);
        assert_eq!(poll_ms(Duration::from_micros(50_001)), 51);
        assert_eq!(poll_ms(DEBOUNCE), 50);
        assert_eq!(poll_ms(Duration::from_secs(u64::MAX)), i32::MAX);
    }

    #[test]
    fn event_buffer() {
        let mut b = Vec::new();
        for (wd, mask, name) in [(1i32, libc::IN_MOVED_TO, &b"palette.json\0\0\0\0"[..]), (2, libc::IN_IGNORED, &b""[..])] {
            b.extend(wd.to_ne_bytes());
            b.extend(mask.to_ne_bytes());
            b.extend(0u32.to_ne_bytes());
            b.extend((name.len() as u32).to_ne_bytes());
            b.extend(name);
        }
        let ev = parse_events(&b);
        assert_eq!(ev.len(), 2);
        assert_eq!((ev[0].wd, ev[0].mask, ev[0].name.as_deref()), (1, libc::IN_MOVED_TO, Some(&b"palette.json"[..])));
        assert_eq!((ev[1].wd, ev[1].name.as_deref()), (2, None));
        // abgeschnittener Puffer: nichts erfinden
        assert_eq!(parse_events(&b[..20]).len(), 0);
    }
}
