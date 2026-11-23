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

//! Hinweis nach einem unsauberen Ende des vorigen Boots (Antwort 8 und Aufgabe 9 in `PLAN-diagnose.md`).
//!
//! Der Daemon beantwortet `diag_boot_hint` (die Oberfläche fragt einmal nach ihrem Start; bis W7 Awesome, unter niri
//! ab W4 `dictate-ui`): endete der Boot vor dem laufenden ohne
//! Spur eines geordneten Herunterfahrens (Absturz, Freeze, Stromausfall), zeigt die Oberfläche einmal einen Hinweis im Overlay.
//! Es wird nichts gesammelt und nichts gesendet; geprüft wird nur, was die Probe `boots` ohnehin liefert (die Liste der
//! Boots und das Ende des Journals von Boot −1 samt [`super::probes::ended_cleanly`], siehe
//! [`super::probes::ListedBoots`]).
//!
//! - **Je Boot höchstens einmal.** Der Daemon merkt sich die Kennung des laufenden Boots in `diagnose.json` (State-Ordner,
//!   neben `history.jsonl`; 0600, atomar geschrieben, nur diese Kennung). Ein Neustart der Oberfläche im selben Boot findet
//!   die Kennung und meldet nichts mehr. Ein Boot zählt als geprüft, sobald das Urteil feststeht (sauber, unsauber, kein
//!   Vorgänger); ließ sich das Ende nicht lesen, wird nichts gemerkt, damit der nächste Start es noch einmal versucht.
//! - **Fehlt die Datei oder ist sie kaputt** (kein JSON, falsche Version, keine Kennung, zu groß, Symlink), gilt der Boot als
//!   noch nicht geprüft; das nächste Merken ersetzt die Datei. Ein Hinweis zu viel ist besser als einer zu wenig.
//! - **Erst die Kennung des Kernels, dann die Probe.** Die Oberfläche fragt bei jedem Start (auch beim Neustart im selben Boot).
//!   Der Dienst liest darum zuerst [`PROC_BOOT_ID`] (über [`ProbePaths`], in Tests ein Fixture-Ordner) und vergleicht sie
//!   mit der gemerkten; nur bei einem neuen oder unbekannten Boot läuft die Probe `boots` (Journal von Boot −1). Ist die
//!   Datei nicht lesbar, entscheidet wie bisher die Probe. Gemerkt wird weiter die Kennung aus dem Journal (dieselbe
//!   Kennung, dort ohne Bindestriche; verglichen wird ohne Bindestriche und Groß/klein, [`same_boot`]).
//! - **Nichts im Log außer** `boot hint: shown` bzw. `boot hint: not needed` (und ein Satz, wenn das Merken scheitert).
//!   Die Kennung des Boots wird nicht geloggt.

use super::probes::fs::ProbePaths;
use super::probes::{ListedBoots, PreviousBoot};
use crate::palette::sources::frecency::write_atomic;
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

/// Version der Datei.
pub const STATE_VERSION: u32 = 1;
/// So groß darf die Datei höchstens sein; sie hat eine Kennung.
const MAX_STATE_BYTES: u64 = 4096;

/// Ist das eine Boot-Kennung? Das Journal nennt sie als 32 Hexziffern; die Form mit Bindestrichen (36 Zeichen) nimmt die
/// Datei auch an.
pub fn valid_boot_id(id: &str) -> bool {
    matches!(id.len(), 32 | 36) && id.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-') && id.bytes().any(|b| b != b'-')
}

/// Kennung des laufenden Boots im Kernel (`8-4-4-4-12` Hexziffern mit Bindestrichen und Zeilenende).
pub const PROC_BOOT_ID: &str = "/proc/sys/kernel/random/boot_id";

/// Eine Boot-Kennung in der Form des Journals: 32 Hexziffern, klein, ohne Bindestriche; sonst `None`.
pub fn normalize_boot_id(raw: &str) -> Option<String> {
    let id = raw.trim();
    if !valid_boot_id(id) {
        return None;
    }
    let hex: String = id.chars().filter(|c| *c != '-').map(|c| c.to_ascii_lowercase()).collect();
    (hex.len() == 32).then_some(hex)
}

/// Dieselbe Kennung? (ohne Bindestriche, Groß/klein egal; ungültige sind nie gleich)
pub fn same_boot(a: &str, b: &str) -> bool {
    matches!((normalize_boot_id(a), normalize_boot_id(b)), (Some(x), Some(y)) if x == y)
}

/// Die Kennung des laufenden Boots aus [`PROC_BOOT_ID`] unter der Wurzel von `paths` (Produktion `/`, Tests ein
/// Fixture-Ordner). `None`, wenn die Datei fehlt, keine reguläre Datei ist oder keine Kennung enthält.
pub fn read_boot_id(paths: &ProbePaths) -> Option<String> {
    let mut f = std::fs::OpenOptions::new().read(true).custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK).open(paths.under_root(PROC_BOOT_ID)).ok()?;
    if !f.metadata().ok()?.is_file() {
        return None;
    }
    let mut text = String::new();
    f.by_ref().take(128).read_to_string(&mut text).ok()?;
    normalize_boot_id(&text)
}

/// Ist der laufende Boot (Kennung des Kernels) schon geprüft? Dann braucht es die Probe `boots` nicht.
pub fn already_checked(kernel: Option<&str>, remembered: Option<&str>) -> bool {
    matches!((kernel, remembered), (Some(k), Some(r)) if same_boot(k, r))
}

/// Warum der Hinweis erscheint oder nicht (nur für die Antwort an den Client und die Tests; kein Inhalt).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Why {
    /// Der Boot davor endete ohne geordnetes Herunterfahren.
    Unclean,
    /// Der Boot davor endete sauber.
    Clean,
    /// Es gibt keinen Boot davor im Journal.
    NoPrevious,
    /// Dieser Boot wurde schon geprüft (Neustart der Oberfläche im selben Boot).
    AlreadyChecked,
    /// Das Urteil steht nicht fest (Journal nicht lesbar, Kennung des Boots fehlt).
    Unknown,
}

impl Why {
    pub fn as_str(self) -> &'static str {
        match self {
            Why::Unclean => "unclean",
            Why::Clean => "clean",
            Why::NoPrevious => "no_previous",
            Why::AlreadyChecked => "already_checked",
            Why::Unknown => "unknown",
        }
    }
}

/// Die Entscheidung zu einer Prüfung.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decision {
    /// Den Hinweis jetzt zeigen.
    pub show: bool,
    /// Diese Boot-Kennung merken (`None` = nichts merken).
    pub remember: Option<String>,
    pub why: Why,
}

/// Reine Entscheidung aus dem, was `boots` sah, und der gemerkten Kennung.
pub fn decide(boots: Option<&ListedBoots>, remembered: Option<&str>) -> Decision {
    let unknown = Decision { show: false, remember: None, why: Why::Unknown };
    let Some(ListedBoots { current_id: Some(current), previous }) = boots else { return unknown };
    if !valid_boot_id(current) {
        return unknown;
    }
    if remembered.is_some_and(|r| same_boot(r, current)) {
        return Decision { show: false, remember: None, why: Why::AlreadyChecked };
    }
    let (show, why) = match previous {
        PreviousBoot::Unclean => (true, Why::Unclean),
        PreviousBoot::Clean => (false, Why::Clean),
        PreviousBoot::None => (false, Why::NoPrevious),
        PreviousBoot::Unknown => return unknown,
    };
    Decision { show, remember: Some(current.clone()), why }
}

#[derive(Serialize, Deserialize)]
struct StateFile {
    version: u32,
    /// Kennung des Boots, für den der Hinweis zuletzt geprüft wurde.
    checked_boot: String,
}

/// Die Datei mit der gemerkten Kennung.
#[derive(Debug, Clone)]
pub struct BootHintState {
    path: PathBuf,
}

impl BootHintState {
    pub fn new(path: PathBuf) -> BootHintState {
        BootHintState { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Die gemerkte Kennung; `None`, wenn die Datei fehlt oder nicht brauchbar ist.
    pub fn load(&self) -> Option<String> {
        // `O_NOFOLLOW`: ein Symlink ist keine Datei von uns; nur reguläre Dateien, begrenzt
        let mut f = std::fs::OpenOptions::new().read(true).custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK).open(&self.path).ok()?;
        let meta = f.metadata().ok()?;
        if !meta.is_file() || meta.len() > MAX_STATE_BYTES {
            return None;
        }
        let mut text = String::new();
        f.by_ref().take(MAX_STATE_BYTES).read_to_string(&mut text).ok()?;
        let file: StateFile = serde_json::from_str(&text).ok()?;
        (file.version == STATE_VERSION && valid_boot_id(&file.checked_boot)).then_some(file.checked_boot)
    }

    /// Merkt die Kennung (atomar, 0600, Ordner 0700).
    pub fn store(&self, boot_id: &str) -> std::io::Result<()> {
        if !valid_boot_id(boot_id) {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "not a boot id"));
        }
        let mut json = serde_json::to_string_pretty(&StateFile { version: STATE_VERSION, checked_boot: boot_id.to_string() })?;
        json.push('\n');
        write_atomic(&self.path, json.as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    const BOOT_A: &str = "c9b1d2e3f4a5465798a1b2c3d4e53333";
    const BOOT_B: &str = "7d04a9e4b1c24f5d8a3e6b7c8d9e0f11";

    struct Dir(PathBuf);

    impl Dir {
        fn new(name: &str) -> Dir {
            let p = std::env::temp_dir().join(format!("dictate-boot-hint-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&p);
            std::fs::create_dir_all(&p).unwrap();
            Dir(p)
        }
        fn state(&self) -> BootHintState {
            BootHintState::new(self.0.join("state/dictate/diagnose.json"))
        }
    }

    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn seen(current: Option<&str>, previous: PreviousBoot) -> ListedBoots {
        ListedBoots { current_id: current.map(Into::into), previous }
    }

    #[test]
    fn an_unclean_end_shows_the_hint_once_per_boot() {
        let boots = seen(Some(BOOT_A), PreviousBoot::Unclean);
        let first = decide(Some(&boots), None);
        assert_eq!(first, Decision { show: true, remember: Some(BOOT_A.into()), why: Why::Unclean });
        // Neustart der Oberfläche im selben Boot: gemerkt, nichts mehr
        let again = decide(Some(&boots), Some(BOOT_A));
        assert_eq!(again, Decision { show: false, remember: None, why: Why::AlreadyChecked });
        // der nächste Boot ist ein neuer Fall
        let next = decide(Some(&seen(Some(BOOT_B), PreviousBoot::Unclean)), Some(BOOT_A));
        assert!(next.show && next.remember.as_deref() == Some(BOOT_B));
    }

    #[test]
    fn a_clean_end_or_no_predecessor_shows_nothing_but_is_remembered() {
        for (previous, why) in [(PreviousBoot::Clean, Why::Clean), (PreviousBoot::None, Why::NoPrevious)] {
            let d = decide(Some(&seen(Some(BOOT_A), previous)), None);
            assert_eq!(d, Decision { show: false, remember: Some(BOOT_A.into()), why });
            // und beim zweiten Mal nicht noch einmal geprüft
            assert_eq!(decide(Some(&seen(Some(BOOT_A), previous)), Some(BOOT_A)).why, Why::AlreadyChecked);
        }
    }

    #[test]
    fn an_unreadable_end_or_a_missing_boot_id_decides_nothing_and_remembers_nothing() {
        for boots in [
            None,
            Some(seen(None, PreviousBoot::Unclean)),
            Some(seen(Some("not a boot id"), PreviousBoot::Unclean)),
            Some(seen(Some(BOOT_A), PreviousBoot::Unknown)),
            Some(seen(Some(&BOOT_A.replace('c', "z")), PreviousBoot::Unclean)),
        ] {
            let d = decide(boots.as_ref(), None);
            assert_eq!(d, Decision { show: false, remember: None, why: Why::Unknown }, "{boots:?}");
        }
        // auch mit gemerkter Kennung: ein unlesbares Ende bleibt unentschieden, es gibt keinen falschen Treffer
        assert_eq!(decide(Some(&seen(Some(BOOT_A), PreviousBoot::Unknown)), Some(BOOT_B)).why, Why::Unknown);
    }

    #[test]
    fn the_kernel_boot_id_skips_the_probe_only_for_the_remembered_boot() {
        let dashed = "C9B1D2E3-F4A5-4657-98A1-B2C3D4E53333\n";
        assert_eq!(normalize_boot_id(dashed).as_deref(), Some(BOOT_A));
        assert!(same_boot(dashed, BOOT_A) && !same_boot(dashed, BOOT_B));
        assert!(already_checked(Some(BOOT_A), Some(BOOT_A)));
        for (kernel, remembered) in [(Some(BOOT_A), None), (None, Some(BOOT_A)), (Some(BOOT_B), Some(BOOT_A)), (Some("x"), Some("x")), (None, None)] {
            assert!(!already_checked(kernel, remembered), "{kernel:?} {remembered:?}");
        }
        // gemerkt mit Bindestrichen (die Datei nimmt beide Formen an): derselbe Boot
        assert_eq!(decide(Some(&seen(Some(BOOT_A), PreviousBoot::Unclean)), Some(dashed.trim())).why, Why::AlreadyChecked);
        // aus einem Fixture-Ordner gelesen
        let t = Dir::new("proc");
        let paths = ProbePaths::rooted(t.0.clone(), t.0.join("home"), Some(t.0.join("templog"))).unwrap();
        assert_eq!(read_boot_id(&paths), None, "missing");
        let p = paths.under_root(PROC_BOOT_ID);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, dashed).unwrap();
        assert_eq!(read_boot_id(&paths).as_deref(), Some(BOOT_A));
        std::fs::write(&p, "garbage\n").unwrap();
        assert_eq!(read_boot_id(&paths), None, "no boot id in it");
        std::fs::remove_file(&p).unwrap();
        std::fs::create_dir(&p).unwrap();
        assert_eq!(read_boot_id(&paths), None, "a directory");
    }

    #[test]
    fn boot_ids_are_hex_of_the_journal_or_with_dashes() {
        assert!(valid_boot_id(BOOT_A));
        assert!(valid_boot_id("c9b1d2e3-f4a5-4657-98a1-b2c3d4e53333"));
        for bad in [
            "",
            "abc",
            "g9b1d2e3f4a5465798a1b2c3d4e53333",
            &"a".repeat(33),
            &"a".repeat(64),
            &"-".repeat(32),
            "c9b1d2e3f4a5465798a1b2c3d4e5333 ",
            "../etc/passwd",
        ] {
            assert!(!valid_boot_id(bad), "{bad:?}");
        }
    }

    #[test]
    fn the_state_file_holds_only_the_boot_id_with_owner_only_permissions() {
        let t = Dir::new("roundtrip");
        let s = t.state();
        assert_eq!(s.load(), None, "no file yet");
        s.store(BOOT_A).unwrap();
        assert_eq!(s.load(), Some(BOOT_A.to_string()));
        let text = std::fs::read_to_string(s.path()).unwrap();
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v, serde_json::json!({"version": 1, "checked_boot": BOOT_A}), "only the version and the ID: {text}");
        assert_eq!(std::fs::metadata(s.path()).unwrap().permissions().mode() & 0o777, 0o600);
        assert_eq!(std::fs::metadata(s.path().parent().unwrap()).unwrap().permissions().mode() & 0o777, 0o700);
        // ein neuer Boot ersetzt die Kennung, ohne Rest
        s.store(BOOT_B).unwrap();
        assert_eq!(s.load(), Some(BOOT_B.to_string()));
        let leftovers: Vec<_> = std::fs::read_dir(s.path().parent().unwrap()).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        assert_eq!(leftovers, ["diagnose.json"], "no temporary file stays behind");
        // etwas anderes als eine Kennung wird nicht geschrieben
        assert!(s.store("../../evil").is_err());
        assert_eq!(s.load(), Some(BOOT_B.to_string()));
    }

    #[test]
    fn a_missing_or_broken_state_file_counts_as_not_checked_and_is_replaced() {
        let t = Dir::new("broken");
        let s = t.state();
        let dir = s.path().parent().unwrap().to_path_buf();
        std::fs::create_dir_all(&dir).unwrap();
        let big = format!("{{\"version\":1,\"checked_boot\":\"{BOOT_A}\",\"pad\":\"{}\"}}", "x".repeat(5000));
        for content in [
            "".to_string(),
            "not json".to_string(),
            "{}".to_string(),
            "[]".to_string(),
            format!("{{\"version\":2,\"checked_boot\":\"{BOOT_A}\"}}"),
            "{\"version\":1}".to_string(),
            "{\"version\":1,\"checked_boot\":\"nonsense\"}".to_string(),
            "{\"version\":1,\"checked_boot\":7}".to_string(),
            big,
            "\u{0}\u{0}\u{0}".to_string(),
        ] {
            std::fs::write(s.path(), &content).unwrap();
            assert_eq!(s.load(), None, "{:?}", &content[..content.len().min(60)]);
            // dann zeigt ein unsauberes Ende den Hinweis, und das Merken ersetzt die Datei
            let d = decide(Some(&seen(Some(BOOT_A), PreviousBoot::Unclean)), s.load().as_deref());
            assert!(d.show);
            s.store(d.remember.as_deref().unwrap()).unwrap();
            assert_eq!(s.load(), Some(BOOT_A.to_string()));
        }
        // ein Ordner oder ein Symlink an der Stelle der Datei: nicht lesen, nicht folgen
        std::fs::remove_file(s.path()).unwrap();
        std::fs::create_dir(s.path()).unwrap();
        assert_eq!(s.load(), None);
        std::fs::remove_dir(s.path()).unwrap();
        let target = t.0.join("elsewhere.json");
        std::fs::write(&target, format!("{{\"version\":1,\"checked_boot\":\"{BOOT_A}\"}}")).unwrap();
        std::os::unix::fs::symlink(&target, s.path()).unwrap();
        assert_eq!(s.load(), None, "a symlink is not followed");
        // schreiben ersetzt den Symlink, statt durch ihn in die fremde Datei zu schreiben
        s.store(BOOT_B).unwrap();
        assert_eq!(s.load(), Some(BOOT_B.to_string()));
        assert!(std::fs::read_to_string(&target).unwrap().contains(BOOT_A), "the file behind the link is untouched");
        assert_eq!(std::fs::metadata(s.path()).unwrap().nlink(), 1);
    }

    #[test]
    fn the_state_cannot_be_written_in_a_directory_that_is_a_file() {
        let t = Dir::new("unwritable");
        std::fs::write(t.0.join("state"), "x").unwrap();
        assert!(t.state().store(BOOT_A).is_err());
    }
}
