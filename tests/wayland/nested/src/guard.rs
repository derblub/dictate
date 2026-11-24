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

//! Wächter von `nested-inject` und `nested-clip`: beide laufen nur im Laufzeitordner des Harness
//! (`tests/wayland/nested/harness.sh`), nie gegen die echte Sitzung.
//!
//! Regel (PLAN-lizenz.md, O7): `DICTATE_TEST_ROOT` muss gesetzt sein (der Harness exportiert ihn aufgelöst; hier gibt
//! es bewusst keine Vorgabe, weil der Harness `XDG_CACHE_HOME` umbiegt), absolut, ohne leere, `.`- und `..`-Teile
//! und nur aus `A-Z a-z 0-9 . _ - /` (wie `scripts/test-root.sh`). `XDG_RUNTIME_DIR` muss unter
//! `<root>/wl-run/` liegen und selbst sauber sein; ein absolutes `WAYLAND_DISPLAY` ebenso. Weil kein `..` durchgeht,
//! kann ein Laufzeitordner nur unter `<root>/wl-run/` liegen, wenn ein Ordner `wl-run` darüber liegt; der Ordner
//! der echten Sitzung (`/run/user/<uid>`) hat keinen, gleich welcher Root eingestellt ist.

/// Prüft die Umgebung des Prozesses (siehe Modulkopf).
pub fn from_env() -> Result<(), String> {
    let get = |k: &str| std::env::var(k).ok();
    check(get("DICTATE_TEST_ROOT").as_deref(), get("XDG_RUNTIME_DIR").as_deref(), get("WAYLAND_DISPLAY").as_deref())
}

/// Reine Prüfung der drei Werte (`None` = nicht gesetzt).
pub fn check(root: Option<&str>, runtime_dir: Option<&str>, wayland_display: Option<&str>) -> Result<(), String> {
    let root = root.filter(|r| !r.is_empty()).ok_or("refusing to run: DICTATE_TEST_ROOT is not set (start through the harness)")?;
    if !clean_absolute(root) {
        return Err(format!("refusing to run: DICTATE_TEST_ROOT {root:?} is not a clean absolute path"));
    }
    let base = format!("{root}/wl-run/");
    let rt = runtime_dir.unwrap_or_default();
    if !clean_absolute(rt) || !rt.starts_with(&base) {
        return Err(format!("refusing to run: XDG_RUNTIME_DIR is not under {base}"));
    }
    if let Some(wl) = wayland_display.filter(|w| w.starts_with('/')) {
        if !clean_absolute(wl) || !wl.starts_with(&base) {
            return Err("refusing to run: WAYLAND_DISPLAY outside the test dir".into());
        }
    }
    Ok(())
}

/// Absolut, mindestens ein Teil, kein leerer, `.`- oder `..`-Teil, nur `A-Z a-z 0-9 . _ - /`.
fn clean_absolute(p: &str) -> bool {
    let Some(rest) = p.strip_prefix('/') else { return false };
    p.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-' | b'/'))
        && rest.split('/').all(|c| !c.is_empty() && c != "." && c != "..")
}

#[cfg(test)]
mod tests {
    use super::check;

    const ROOT: &str = "/home/alice/.cache/dictate-test";
    const RT: &str = "/home/alice/.cache/dictate-test/wl-run/n.AbC123/rt";

    #[test]
    fn harness_layout_passes() {
        assert_eq!(check(Some(ROOT), Some(RT), Some("wayland-1")), Ok(()));
        assert_eq!(check(Some(ROOT), Some(RT), None), Ok(()));
        assert_eq!(check(Some(ROOT), Some(RT), Some(&format!("{RT}/wayland-1"))), Ok(()));
        assert_eq!(check(Some("/home/data/stt-test"), Some("/home/data/stt-test/wl-run/n.x/rt"), None), Ok(()));
    }

    #[test]
    fn real_session_is_refused_for_any_root() {
        for root in [ROOT, "/run/user/1000", "/run/user", "/run", "/run/user/1000/wl-run"] {
            assert!(check(Some(root), Some("/run/user/1000"), None).is_err(), "{root}");
            assert!(check(Some(root), Some(RT), Some("/run/user/1000/wayland-1")).is_err(), "{root}");
        }
    }

    #[test]
    fn root_must_be_set_and_clean() {
        assert!(check(None, Some(RT), None).is_err());
        assert!(check(Some(""), Some(RT), None).is_err());
        for root in ["relative/x", "/", "/a/../b", "/a/./b", "/a//b", "/a/", "/a b", "/ä", "/x+y"] {
            assert!(check(Some(root), Some(&format!("{root}/wl-run/n.x/rt")), None).is_err(), "{root}");
        }
    }

    #[test]
    fn runtime_dir_must_be_below_wl_run_and_clean() {
        assert!(check(Some(ROOT), None, None).is_err());
        assert!(check(Some(ROOT), Some(""), None).is_err());
        assert!(check(Some(ROOT), Some(&format!("{ROOT}/wl-run")), None).is_err());
        assert!(check(Some(ROOT), Some(&format!("{ROOT}/wl-run-x/n.x/rt")), None).is_err());
        assert!(check(Some(ROOT), Some(&format!("{ROOT}/wl-runx")), None).is_err());
        assert!(check(Some(ROOT), Some(&format!("{ROOT}/other/n.x/rt")), None).is_err());
        assert!(check(Some(ROOT), Some(&format!("{ROOT}/wl-run/../../../../../run/user/1000")), None).is_err());
        assert!(check(Some(ROOT), Some(&format!("{ROOT}/wl-run/./n.x")), None).is_err());
        assert!(check(Some(ROOT), Some(&format!("{ROOT}/wl-run//n.x")), None).is_err());
    }

    #[test]
    fn absolute_wayland_display_must_be_below_wl_run() {
        assert!(check(Some(ROOT), Some(RT), Some(&format!("{ROOT}/wl-run/../x/wayland-1"))).is_err());
        assert!(check(Some(ROOT), Some(RT), Some("/tmp/wayland-1")).is_err());
    }
}
