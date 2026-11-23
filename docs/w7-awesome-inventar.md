# W7: Aufstellung „AwesomeWM entfernen“ (zur Freigabe durch Daniel)

Stand 20.11.2026, Branch `wayland-w3` (nach W3). Erstellt nur lesend (Agent, etwa 6 min), Zeilen und Stellen aus dem
Repo; **unklar** markiert, was am Code nicht entscheidbar war. Entscheidung 15 in `PLAN-wayland.md`: „Alles was
AwesomeWM ist, kann weg“, vorher diese Aufstellung, gelöscht wird in einem eigenen Block (6f).

## Zusammenfassung

- **Nur Awesome:** `awesome/` mit 15 097 Zeilen (12 Lua-Module mit 10 764, `awesome/test` 36 Dateien mit 4 333), dazu
  Rust nur für das Lua-Modul: `--format awesome` (`Format::Awesome`, `to_awesome`, `esc`).
- **X11 außerhalb Awesome:** `src/keys.rs` (159), `src/paste.rs` (664, ganz X), `Xclip` in `src/actions/selection.rs`
  (etwa 300 von 334), Abhängigkeit `x11rb`, eframe-Feature `x11`, `tests/paste_x.rs`, fast ganz `tests/act_cli.rs`,
  `tests/gui/*` (Xvfb/xdotool für das bleibende `dictate-settings`).
- **Vorher umziehen:** `awesome/test/fake-logind` nutzen `tests/wayland/run.sh` und `lock.sh`.
- **Nicht ganz löschbar:** `src/actions/selection.rs`: `MAX_BYTES` und `TIMEOUT` brauchen `dictate-ui/primary.rs` und
  `client.rs`.
- **README** nennt `dictate-ui` und niri gar nicht.
- **Ohne Aufrufer danach:** `dictate-palette`, `dictate-diag`, `dictate act`, `dictate paste`, `release-keys`, `synth`,
  `listen` (`dictate-ui` spricht den Socket direkt; W4/W5 sollen das auch).
- **Einstellungsfenster** hat schon einen niri-Weg (Mod+Ctrl+Shift+E, `open_settings`); es fehlt eine Fensterregel
  (schwebend) in `niri/dictate-binds.kdl`.
- **Ohne niri-Ersatz bis W4/W5:** Befehlspalette (W5), Diagnose-Panel samt Boot-Hinweis und Mod+Shift+I (W4),
  Leistensymbol `widget.lua` (offen), Stimme in die Palette.

## (A) Nur Awesome

| Pfad | Zeilen | Zweck / Ersatz |
|---|---|---|
| `awesome/dictate/init.lua` | 1808 | Overlay, `listen`, Halten, Tipp-Warteschlange, Paste, Stimmenziel, Boot-Hinweis, Tasten. Ersatz: `dictate-ui` (W1/W2) |
| `awesome/dictate/actions.lua` | 2888 | Panel Textaktionen, Hülle der Diagnose. Ersatz: W3 |
| `awesome/dictate/diagnose.lua` | 2265 | Diagnose-Panel. Kein Ersatz (W4) |
| `awesome/dictate/palette.lua` | 1912 | Befehlspalette. Kein Ersatz (W5) |
| `awesome/dictate/palette_exec.lua` | 768 | Ausführung der Palette-Absichten. Kein Ersatz (W5) |
| `awesome/dictate/lock.lua` | 150 | Sperre. Ersetzt (`src/ui/lock.rs`, `dictate-ui/lock.rs`) |
| `awesome/dictate/markdown.lua` | 236 | Portiert (`src/ui/markdown.rs`, Testfälle übernommen) |
| `awesome/dictate/json.lua` | 316 | JSON für den Palette-Kontext; W5 braucht `sanitize` als Vorlage |
| `awesome/dictate/palette_url.lua` | 84 | URL-Prüfung; Vorlage für W5 |
| `awesome/dictate/style.lua` | 106 | Schriften |
| `awesome/dictate/drag.lua` | 153 | Ziehen mit Mod+Maus; entfällt unter niri |
| `awesome/dictate/widget.lua` | 78 | Leistensymbol; kein Ersatz |
| `awesome/test/run.sh` | 730 | Xephyr-Hauptlauf |
| `awesome/test/palette*.sh` | 1339 | Palette-Tests (6 Dateien) |
| `awesome/test/diagnose.sh`, `release.sh`, `shots.sh` | 527 | Xephyr-Tests |
| `awesome/test/rc.lua`, `entry.py`, `backdrop.py` | 233 | Test-WM und Hilfen |
| `awesome/test/fake-dictate*`, `fake-settings` | 1135 | Fake-CLIs |
| `awesome/test/*_test.lua` | 191 | Lua-Unit-Tests (`palette_url_test.lua` Vorlage für W5) |
| `awesome/test/bin/` | – | Fake-Programme |
| `awesome/test/fake-logind` | 111 | **umziehen** nach `tests/wayland/` |
| `awesome/test/xkeys` | 37 | nur `tests/paste_x.rs` |
| `src/main.rs` `enum Format`, `--format` bei `act`/`listen` | | CLI-Schalter `json\|awesome` |
| `src/client.rs` `Format::Awesome`, `listen_at`-Arm, `grab_awesome`, `act_fail`, `act_refused`, `act_unreachable`, Awesome-Arme, 2 Tests; `preview()` (**unklar**, ob das JSON-Format es nutzt) | | Awesome-Zeilenformat |
| `src/protocol.rs` `esc`, `to_awesome`, Test | | Zeilenformat für Lua |
| `Cargo.toml` `description` | | nennt AwesomeWM |
| `tests/daemon_client.rs` (2 Tests), `tests/daemon_protocol.rs` (`listen_awesome_format_…`), `tests/daemon_pipeline.rs` (`to_awesome`) | | Format-Tests |

## (B) X11 außerhalb Awesome

| Pfad / Symbol | Zweck | Aufrufer |
|---|---|---|
| `src/keys.rs` (159) | XInput/XTest: `release_synthetic`, `wait_released`, … | `main.rs` (`release-keys`, `synth`), `paste.rs`; danach keiner |
| `src/paste.rs` (664) | X11-Zwischenablage + `xdotool ctrl+v` | `main.rs`, `tests/paste_x.rs`; danach keiner |
| `src/actions/selection.rs` | `Xclip`, `SelError`, `decode`, `DICTATE_XCLIP`; **bleiben:** `TIMEOUT`, `MAX_BYTES` | `client.rs` (act grab/copy), `diagnose/client.rs` (`diag copy`) |
| `src/main.rs` | Unterbefehle `paste`, `release-keys`, `synth` | nur Lua |
| `Cargo.toml` `x11rb` (`xinput`, `xtest`) | nur `keys.rs`, `paste.rs`; transitiv über eframe/winit **unklar** | |
| `Cargo.toml` eframe-Feature `x11` | winit-X11 für `dictate-settings`; streichen bricht `tests/gui` (Xvfb) | |
| `tests/paste_x.rs` (362), `tests/act_cli.rs` (677, fast ganz), `tests/diag_cli.rs` (Fake-xclip-Teile) | X-Tests | Schritte `unit`, `xclip` |
| `tests/gui/*` (634) | Xvfb :98, xdotool; testet das bleibende `dictate-settings` | Schritt `gui` |
| `scripts/check.sh` | Schritte `xclip`, `xephyr`, `XEPHYR_*`; `gui` auf Xvfb | |
| `src/diagnose` Probe `xorg_log` (`XorgLog`/`XorgBoot`, Profil `last_crash`) | unter niri kein `Xorg.0.log` | **Entscheidung** |
| `overlay.transparency` (auto/argb/window/none) | X-Begriffe; `dictate-ui` behandelt alles außer `none` gleich | **Entscheidung** |
| Einstellungsfenster „xprop WM_CLASS“ (`gui/pages/actions.rs`, `profiles.rs`) | unter niri `app_id` (`niri msg windows`) | Textänderung |
| `WM_CLASS` in Kommentaren/Namen (`config.rs`, `profile.rs`, `main.rs`, `client.rs`, `protocol.rs`, `daemon.rs`) | Wortlaut | Logik geteilt |
| `terminal_classes`-Vorgabe | **bleibt** (XWayland-Programme melden dort ihre Klasse) | |
| `packaging/dictate-settings.desktop` `StartupWMClass` | unter Wayland nötig? **unklar** | |
| `tests/wayland/*` (xterm, xwayland-satellite) | XWayland-Gegenprobe des Wayland-Wegs; **bleibt** | |

## (C) Gemeinsam genutzt, bleibt

`src/protocol.rs` ohne `esc`/`to_awesome`; `sock_client`, `config`, `actions/` (`store`, `secrets`, `diff`, `markdown`,
`wordlist`); `Event::Overlay`/`Terminals`/`Final.keys` (dictate-ui); `Request::Start`/`Toggle { app }`; Daemon `act_*`,
`diag_clear`, `pal_sources_refresh`, `pal_frecency_clear` (Einstellungsfenster); `src/palette/`, `src/diagnose/` (ohne
`xorg_log`); `src/ui/*`, `src/bin/dictate-ui/*`; `dictate-settings`, `src/gui/*`; `niri/`, `noctalia/`, `systemd/`;
`tests/daemon_*.rs`, `tests/wayland/*`, `tests/diagnose/corpus`, `tests/speech*`.

## (D) Aufrufer fallen weg

| Ebene | Symbol | Heute | Danach |
|---|---|---|---|
| CLI | `listen --format awesome` | `init.lua` | keiner |
| CLI | `listen` (json) | keiner im Repo | nur Debug (**unklar**) |
| CLI | `paste`, `release-keys`, `synth` | `init.lua` | keiner |
| CLI | `act grab\|run\|cancel\|copy\|clear` | `actions.lua`, `act_cli.rs` | keiner (dictate-ui direkt) |
| CLI | `palette …`, `dictate-palette` | `palette.lua` | keiner bis W5 |
| CLI | `diag …`, `dictate-diag` | `diagnose.lua`, Boot-Hinweis | keiner (W4 direkt) |
| CLI | `start\|stop\|toggle\|cancel\|status\|shutdown` | `init.lua` | nur von Hand |
| Daemon | `pal_resolve`, `pal_ask`, `pal_chosen`, `PaletteVoice`, `act_grab` mit `clipboard\|question` | Palette-Lua | bis W5 ohne Aufrufer, **bleibt** |
| Daemon | `diag_plan … diag_boot_hint` | `dictate-diag` | bis W4 ohne Aufrufer, **bleibt** |

## (E) Ohne niri-Ersatz bis W4/W5

Befehlspalette (W5; Awesome-Absichten `view_tag`, `move_window_to_tag`, `set_layout`, `previous_tag`, `toggle_window`,
`focus_window`, `show_widget` und `ClientInfo.id` als X-Fenster-ID brauchen Entscheidungen, 3.7), Stimme in die Palette
(W5), `explain_clipboard`/`ask`/`text_action` (W5), Diagnose-Panel samt Copy report über Wayland (W4), Boot-Hinweis
(W4), Leistensymbol (offen). Einstellungsfenster: vorhanden, es fehlt die Fensterregel.

## (F) Doku und sichtbare Texte

- `README.md` (ganz auf Awesome; `dictate-ui`/niri fehlen), `CLAUDE.md` (viele Stellen, u. a. Fallen release-keys/synth,
  picom), `DESIGN.md`, `ROADMAP.md`, `PLAN-wayland.md` (W7-Zeile, 6b); historische Pläne (`SPEC-v2.md`, `PLAN.md`,
  `PLAN-actions.md`, `PLAN-palette.md`, `PLAN-diagnose.md`, `UX-actions.md`, `diagnose-ai.md`): Archiv-Vermerk statt
  Umschreiben.
- Rust-Doku und Hilfetexte (`main.rs`, `protocol.rs`, `config.rs`, `daemon.rs`, `diagnose/`, `palette/`, `ui/`,
  `dictate-ui`, `systemd/dictate-ui.service`).
- **Sichtbar (Verhalten):** Einstellungsfenster „Set in the Awesome module. Read only here.“ mit Mod-Tabellen
  (`gui/pages/general.rs`, `actions.rs`, `palette.rs`, `diagnose.rs` „when Awesome starts“); KI-Prompts
  `diagnose/ai.rs` („Arch Linux, X11, AwesomeWM“) und `palette/ai.rs` („AwesomeWM“).
- **Außerhalb des Repos (chezmoi):** Symlink `~/.config/awesome/dictate` und `rc.lua`-Block, Theme-Segment,
  Menüeintrag, `~/.config/picom/picom.conf` (`_DICTATE_OVERLAY@`), `~/.xinitrc` (`awesome --no-argb`), installierte
  `dictate-palette`/`dictate-diag` in `~/.cargo/bin`.
