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

//! Ohne Daemon: TOML direkt lesen und schreiben. Validierung, Kommentar erhaltendes Schreiben (toml_edit),
//! atomares Ersetzen und Modus 0600 kommen aus `config.rs` (dieselbe Logik wie im Daemon).

use super::dto::Settings;
use std::path::{Path, PathBuf};

/// Fehler mit optionalem Feldpfad (`ai.timeout_ms`).
#[derive(Debug, Clone)]
pub struct FieldError {
    pub field: Option<String>,
    pub msg: String,
}

pub fn config_path() -> PathBuf {
    crate::config::Config::default_path()
}

/// Liest die TOML (fehlt sie, gelten die Defaults) als `Settings`. Eine fehlende Datei fällt nicht auf den
/// Standardpfad zurück (sonst läse ein Test oder `--config` die echte Konfiguration).
pub fn load(path: &Path) -> Result<Settings, String> {
    let cfg = if path.exists() { crate::config::Config::load(Some(path)).map_err(|e| format!("{e:#}"))? } else { crate::config::Config::default() };
    Settings::from_value(cfg.to_json())
}

/// Prüft mit der Validierung aus `config.rs` und schreibt die Datei.
pub fn save(path: &Path, s: &Settings) -> Result<(), FieldError> {
    let cfg = crate::config::Config::from_value(s.to_value())
        .and_then(|c| c.validate_new().map(|()| c))
        .map_err(|e| FieldError { field: (!e.field.is_empty()).then_some(e.field), msg: e.msg })?;
    cfg.save(path).map_err(|e| FieldError { field: None, msg: format!("{e:#}") })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("dictate-gui-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d.join("config.toml")
    }

    #[test]
    fn save_keeps_comments_and_writes_only_changes() {
        let p = tmp("keep");
        std::fs::write(&p, "# mein Kommentar\nthreads = 4\n").unwrap();
        let mut s = load(&p).unwrap();
        s.language = "en".into();
        s.overlay.max_width = 900;
        s.vocabulary.replacements = vec![("post gress".into(), "PostgreSQL".into())];
        save(&p, &s).unwrap();
        let out = std::fs::read_to_string(&p).unwrap();
        assert!(out.contains("# mein Kommentar"));
        assert!(out.contains("language = \"en\""));
        assert!(out.contains("max_width = 900"));
        assert!(out.contains("post gress"));
        assert!(!out.contains("step_ms"), "{out}");
        let back = load(&p).unwrap();
        assert_eq!(back, s);
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&p).unwrap().permissions().mode() & 0o777, 0o600);
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
    }

    #[test]
    fn save_roundtrips_profiles_and_modes() {
        let p = tmp("prof");
        let mut s = Settings::default();
        s.profiles.push(crate::gui::dto::Profile { name: "term".into(), match_class: vec!["kitty".into()], ..Default::default() });
        s.ai.modes.retain(|(k, _)| k != "email");
        s.features.history = true;
        save(&p, &s).unwrap();
        let back = load(&p).unwrap();
        assert_eq!(back.profiles, s.profiles);
        assert!(back.features.history);
        assert_eq!(back.ai.modes.len(), crate::config::BUILTIN_MODES.len() - 1);
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
    }

    #[test]
    fn apply_without_changes_drops_frozen_default_modes() {
        // so hatte das Fenster die eingebauten Modi wortgleich in die Datei geschrieben
        let p = tmp("frozen");
        let mut src = String::from("# mine\n[ai]\npreset = \"anthropic\"\nprovider = \"anthropic\"\n[ai.modes]\n");
        for (name, prompt) in crate::config::BUILTIN_MODES {
            src.push_str(&format!("{name} = \"\"\"\n{prompt}\"\"\"\n"));
        }
        std::fs::write(&p, &src).unwrap();
        let mut s = load(&p).unwrap();
        assert_eq!(s.ai.modes, Settings::default().ai.modes);
        save(&p, &s).unwrap();
        let out = std::fs::read_to_string(&p).unwrap();
        assert!(out.contains("# mine") && !out.contains("[ai.modes]"), "{out}");
        // ein geänderter und ein entfernter Modus: nur diese stehen danach in der Datei
        s.ai.modes.retain(|(k, _)| k != "email");
        s.ai.modes.iter_mut().find(|(k, _)| k == "clean").unwrap().1 = "Fix it.".into();
        save(&p, &s).unwrap();
        let out = std::fs::read_to_string(&p).unwrap();
        assert!(out.contains("removed_modes = [\"email\"]") && out.contains("clean = \"Fix it.\"") && !out.contains("to_de"), "{out}");
        assert_eq!(load(&p).unwrap(), s);
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
    }

    #[test]
    fn model_that_cannot_run_names_the_field() {
        let p = tmp("model");
        let mut s = Settings::default();
        s.features.actions = true;
        s.ai.preset = "anthropic".into();
        s.ai.provider = "anthropic".into();
        s.ai.model = "qwen2.5:7b-instruct".into();
        assert_eq!(save(&p, &s).unwrap_err().field.as_deref(), Some("ai.model"));
        s.ai.model.clear();
        save(&p, &s).unwrap();
        assert!(!std::fs::read_to_string(&p).unwrap().contains("model"));
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
    }

    #[test]
    fn invalid_value_names_the_field_and_writes_nothing() {
        let p = tmp("bad");
        let mut s = Settings::default();
        s.overlay.opacity = 1.5;
        let e = save(&p, &s).unwrap_err();
        assert_eq!(e.field.as_deref(), Some("overlay.opacity"));
        assert!(!p.exists());
        s.overlay.opacity = 0.5;
        s.profiles.push(Default::default());
        let e = save(&p, &s).unwrap_err();
        assert!(e.field.unwrap().contains("profiles"));
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
    }

    #[test]
    fn palette_page_saves_only_changes_and_reads_them_back() {
        let p = tmp("palette-ok");
        std::fs::write(&p, "# mine\n[palette]\nproject_depth = 3 # deeper\n").unwrap();
        let mut s = load(&p).unwrap();
        assert_eq!(s.palette.project_depth, 3);
        s.palette.project_roots = vec!["/home/git".into(), "~/work".into()];
        s.palette.frecency = false;
        s.palette.timeout_ms = 12_000;
        s.palette.web_search_url = "https://example.org/?s={query}".into();
        s.ai.preset = "anthropic".into();
        s.ai.provider = "anthropic".into();
        s.palette.model = "claude-sonnet-5-5".into();
        save(&p, &s).unwrap();
        let out = std::fs::read_to_string(&p).unwrap();
        assert!(out.contains("# mine") && out.contains("project_depth = 3 # deeper"), "{out}");
        for line in
            ["frecency = false", "timeout_ms = 12000", "model = \"claude-sonnet-5-5\"", "web_search_url = \"https://example.org/?s={query}\"", "\"~/work\""]
        {
            assert!(out.contains(line), "{line}: {out}");
        }
        assert!(!out.contains("enabled"), "{out}");
        assert_eq!(load(&p).unwrap(), s);
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
    }

    #[test]
    fn palette_page_errors_name_their_field_and_write_nothing() {
        let p = tmp("palette-bad");
        let field = |f: &dyn Fn(&mut Settings)| {
            let mut s = Settings::default();
            f(&mut s);
            save(&p, &s).unwrap_err().field
        };
        assert_eq!(field(&|s| s.palette.project_roots = vec!["/ok".into(), "relative".into()]).as_deref(), Some("palette.project_roots[1]"));
        assert_eq!(field(&|s| s.palette.project_roots = vec![String::new()]).as_deref(), Some("palette.project_roots[0]"));
        assert_eq!(field(&|s| s.palette.web_search_url = "http://example.org/?q={query}".into()).as_deref(), Some("palette.web_search_url"));
        assert_eq!(field(&|s| s.palette.web_search_url = "https://example.org/".into()).as_deref(), Some("palette.web_search_url"));
        assert_eq!(field(&|s| s.palette.timeout_ms = 500).as_deref(), Some("palette.timeout_ms"));
        assert_eq!(field(&|s| s.palette.project_depth = 5).as_deref(), Some("palette.project_depth"));
        // ein Modell, das der Server nicht anbietet, nur bei eingeschalteter Palette (`validate_new`)
        let wrong_model = |s: &mut Settings| {
            s.ai.preset = "anthropic".into();
            s.ai.provider = "anthropic".into();
            s.palette.model = "qwen2.5:7b-instruct".into();
        };
        assert_eq!(field(&wrong_model).as_deref(), Some("palette.model"));
        assert_eq!(field(&|s| s.palette.project_depth = 0).as_deref(), Some("palette.project_depth"));
        assert!(!p.exists(), "nothing is written for an invalid page");
        let mut s = Settings::default();
        wrong_model(&mut s);
        s.palette.enabled = false;
        save(&p, &s).unwrap();
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
    }

    #[test]
    fn diagnose_page_saves_only_changes_and_reads_them_back() {
        let p = tmp("diagnose-ok");
        std::fs::write(&p, "# mine\n[diagnose]\nrounds = 2 # fewer\n").unwrap();
        let mut s = load(&p).unwrap();
        assert_eq!((s.diagnose.rounds, s.diagnose.enabled), (2, false));
        s.diagnose.enabled = true;
        s.diagnose.timeout_ms = 120_000;
        s.diagnose.report_kib = 64;
        s.diagnose.temp_log_dir = "~/temps".into();
        s.diagnose.ping_targets = vec![crate::config::PingTargetEntry { name: "nas".into(), address: "nas.example.org".into() }];
        s.diagnose.pseudonymize = false;
        s.diagnose.boot_hint = false;
        s.ai.preset = "anthropic".into();
        s.ai.provider = "anthropic".into();
        s.diagnose.model = "claude-haiku-5-5".into();
        save(&p, &s).unwrap();
        let out = std::fs::read_to_string(&p).unwrap();
        assert!(out.contains("# mine") && out.contains("rounds = 2 # fewer"), "{out}");
        for line in [
            "enabled = true",
            "timeout_ms = 120000",
            "report_kib = 64",
            "temp_log_dir = \"~/temps\"",
            "pseudonymize = false",
            "boot_hint = false",
            "model = \"claude-haiku-5-5\"",
            "address = \"nas.example.org\"",
        ] {
            assert!(out.contains(line), "{line}: {out}");
        }
        assert_eq!(load(&p).unwrap(), s);
        // zurück auf die Vorgaben: die Schlüssel, die schon in der Datei standen, behalten ihren Platz mit dem Wert der
        // Vorgabe; die Liste der Ziele entfällt (keine leere Liste `ping_targets = []` als Altlast)
        let mut back = load(&p).unwrap();
        back.diagnose = crate::config::DiagnoseCfg::default();
        save(&p, &back).unwrap();
        let out = std::fs::read_to_string(&p).unwrap();
        assert!(!out.contains("ping_targets") && out.contains("# mine") && out.contains("rounds = 3 # fewer"), "{out}");
        assert_eq!(load(&p).unwrap().diagnose, crate::config::DiagnoseCfg::default());
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
    }

    #[test]
    fn diagnose_page_errors_name_their_field_and_write_nothing() {
        let p = tmp("diagnose-bad");
        let target = |name: &str, address: &str| crate::config::PingTargetEntry { name: name.into(), address: address.into() };
        let field = |f: &dyn Fn(&mut Settings)| {
            let mut s = Settings::default();
            f(&mut s);
            save(&p, &s).unwrap_err().field
        };
        assert_eq!(field(&|s| s.diagnose.timeout_ms = 1000).as_deref(), Some("diagnose.timeout_ms"));
        assert_eq!(field(&|s| s.diagnose.report_kib = 7).as_deref(), Some("diagnose.report_kib"));
        assert_eq!(field(&|s| s.diagnose.report_kib = 193).as_deref(), Some("diagnose.report_kib"));
        assert_eq!(field(&|s| s.diagnose.rounds = 0).as_deref(), Some("diagnose.rounds"));
        assert_eq!(field(&|s| s.diagnose.rounds = 6).as_deref(), Some("diagnose.rounds"));
        assert_eq!(field(&|s| s.diagnose.temp_log_dir = "relative".into()).as_deref(), Some("diagnose.temp_log_dir"));
        assert_eq!(field(&|s| s.diagnose.temp_log_dir = "/a/../b".into()).as_deref(), Some("diagnose.temp_log_dir"));
        assert_eq!(field(&|s| s.diagnose.ping_targets = vec![target("nas", "192.0.2.1"), target("", "")]).as_deref(), Some("diagnose.ping_targets[1].name"));
        assert_eq!(field(&|s| s.diagnose.ping_targets = vec![target("nas", "-oProxy")]).as_deref(), Some("diagnose.ping_targets[0].address"));
        assert_eq!(
            field(&|s| s.diagnose.ping_targets = vec![target("nas", "192.0.2.1"), target("NAS", "192.0.2.2")]).as_deref(),
            Some("diagnose.ping_targets[1].name")
        );
        // ein Modell, das der Server nicht anbietet, nur bei eingeschalteter Diagnose (`validate_new`)
        let wrong_model = |s: &mut Settings| {
            s.ai.preset = "anthropic".into();
            s.ai.provider = "anthropic".into();
            s.diagnose.model = "qwen2.5:7b-instruct".into();
        };
        let mut s = Settings::default();
        wrong_model(&mut s);
        save(&p, &s).unwrap();
        s.diagnose.enabled = true;
        assert_eq!(save(&p, &s).unwrap_err().field.as_deref(), Some("diagnose.model"));
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
    }

    #[test]
    fn missing_file_gives_defaults() {
        let p = tmp("none");
        assert_eq!(load(&p).unwrap(), Settings::default());
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
    }
}
