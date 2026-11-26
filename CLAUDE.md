# CLAUDE.md

Lokales Live-Diktat für niri (Wayland), entwickelt auf dem Rechner des Autors (Arch Linux, RTX 3060); Agenten laufen
auf diesem Rechner, „auf dem Rechner des Autors“ meint also die eigene Umgebung. Antworten an den User auf Deutsch,
alle sichtbaren Texte des Tools auf Englisch. Architektur: `PLAN-wayland.md` (niri, `dictate-ui`, Stand je Block), dazu
historisch `PLAN.md` (v1) und `SPEC-v2.md` (Einstellungsfenster, Pipeline, KI; beide mit Archiv-Vermerk), Design:
`DESIGN.md` (Noctalia-Palette für die ganze Anwendung, seit W6 Block E; siehe Fallen), Bedienung: `README.md`.
**Grundplan über das Diktat hinaus** (Textaktionen, Befehlspalette, Linux-Diagnose, Workspaces, Sicherheitsregeln):
`ROADMAP.md`. Textaktionen (Baustein 2): `PLAN-actions.md` (Entscheidungen, Protokoll, Stand). Befehlspalette
(Baustein 3): `PLAN-palette.md`. Linux-Diagnose (Baustein 4): `PLAN-diagnose.md` (Entscheidungen, Protokoll, Stand je
Aufgabe), Messung der Modelle `diagnose-ai.md`. Diese drei Pläne beschreiben den Stand unter AwesomeWM (Archiv-Vermerk);
Protokoll und Daemon-Teile gelten weiter.
Kleine Funktionen und freie Modellwahl über mehrere Engines samt CPU-Weg (Bausteine 7 und 8, nur Plan) sowie die Messung
des GlobalShortcuts-Portals: `PLAN-features.md`.

## Stand (26.11.2026)

- **Phase 6 (Wayland unter niri) abgeschlossen:** W0–W7 samt W6-Nachzug gemergt (`main` `6863c6a`), gepusht,
  installiert und am 23.11. von Daniel live abgenommen (Entscheidung 23). Geschichte, Entscheidungen 1–23 und die
  Liste „nicht belegt, beobachten“: `PLAN-wayland.md` (Kopf, 6h „Live-Abnahme W2–W6“, Abschnitt 7).
- **Aufbau:** `dictate` (`daemon` als Dienst, `transcribe` zum Offline-Test), `dictate-ui` (Overlay als Layer-Fläche,
  Halten über `EVIOCGKEY`, Textausgabe, Panels für Textaktionen, Diagnose und Palette, Sperre über logind),
  `dictate-settings` (Einstellungsfenster, egui, Noctalia-Farben). AwesomeWM und die X11-Wege sind entfernt (W7, auch
  aus chezmoi und `~`; Reste unter `/home/data/archiv/`); `x11rb` bleibt transitiv über arboard.
- **Bausteine** (`ROADMAP.md`): Textaktionen (`act_*`, Seite „Actions“, standardmäßig aus), Befehlspalette (`pal_*`,
  `src/palette/`, Seite „Palette“, standardmäßig an), Linux-Diagnose (`diag_*`, `src/diagnose/` mit 33 lesenden Proben,
  Seite „Diagnose“, standardmäßig aus). Die Pläne `PLAN-actions.md`, `PLAN-palette.md`, `PLAN-diagnose.md`
  beschreiben den Stand unter AwesomeWM (Archiv); Protokoll und Daemon-Teile gelten weiter.
- **Läuft:** User-Dienste `dictate.service` (`~/.cargo/bin/dictate daemon`) und `dictate-ui.service` (`dictate-ui run`,
  nur in einer Wayland-Sitzung); `niri/dictate-binds.kdl` über chezmoi (gepusht am 23.11.).
- **Härtung der Units** (25.11., `docs/systemd-hardening.md`, englisch, weil die Units darauf verweisen):
  `dictate-ui.service` mit Dateisystem-, Kernel- und Netz-Optionen (`systemd-analyze` offline 9.2 → 1.4),
  `dictate.service` nur mit den Optionen ohne Namespace (9.2 → 5.9), weil die Programme der Diagnose als Kinder des
  Daemons laufen und im Namespace Falsches melden. Regeln dazu unter „Fallen“. Installiert sind die neuen Units erst,
  wenn Daniel sie nach `~/.config/systemd/user/` kopiert (nur zusammen mit dem passenden `dictate-ui`: ein altes
  startete das Einstellungsfenster in der Sandbox). Beim nächsten AUR-Release installiert der PKGBUILD zusätzlich
  `docs/systemd-hardening.md` nach `/usr/share/doc/dictate/docs/` (der Tarball von 0.1.0 enthält die Seite nicht).
- **Kürzel** (niri): `Mod+E` halten = sprechen, `Mod+Shift+E` freihändig, `Mod+Ctrl+E` abbrechen, `Mod+Ctrl+Shift+E`
  Einstellungen (schwebend), `Mod+I` Textaktionen, `Mod+Shift+I` Diagnose, `Mod+Alt+Space` Befehlspalette (die Suche
  von espanso auf derselben Taste ist abgeschaltet, siehe Fallen).
- **Repository** (seit U5, `PLAN-lizenz.md`): öffentlich auf GitHub `derblub/dictate` (Arbeitsort, kein Spiegel),
  Verlauf beginnt mit einem Start-Commit (H1). Der Verlauf davor liegt privat im archivierten GitLab-Projekt
  (`git@git.pushingpixels.at:daniel/dictate.git`, nur lesen); Commit-Hashes aus der Zeit vor U5 in Plänen, `SPEC-v2.md`
  und dieser Datei gelten nur dort.
- **Lizenz und Veröffentlichung (U1–U5) abgeschlossen** (26.11.2026; Geschichte, Entscheidungen und Belege:
  `PLAN-lizenz.md`): GPL-3.0-or-later mit Zusatzerlaubnis für NVIDIA CUDA (LICENSE, Lizenzkopf in jeder `.rs`-Datei,
  `THIRD-PARTY-LICENSES.txt`), `CONTRIBUTING.md`, `CLA.md`, `SECURITY.md`, CI auf GitHub. Öffentlich seit 24.11.;
  Tag `v0.1.0` mit GitHub-Release und `dictate 0.1.0-1` im AUR (Konto `derblub`) am 24.11.; GitHub Sponsors mit
  `FUNDING.yml` und README-Link (PR #7); README mit Demo-GIFs (PR #9); Ankündigung am 26.11. von Daniel veröffentlicht.
  Die Rohdaten der Prüfläufe (O5) sind am 26.11. gelöscht, die Werkzeuge privat gesichert. Der U5c-Verlauf
  (`u5-stand`) liegt nur als Bundle unter `/home/data/archiv/`.
- Getestet: Mock-Tests, `dictate-ui` im verschachtelten niri (`tests/wayland/run.sh`), Einstellungsfenster gegen den
  Mock im verschachtelten niri (`tests/gui/`), Ende-zu-Ende über eine virtuelle Pulse-Quelle, Claude API live.

## Offen

1. **Lizenz und Veröffentlichung, Reste** (U1–U5 sind abgeschlossen, siehe „Stand“; `PLAN-lizenz.md`): Die
   Anwaltsfragen bleiben ohne Termin offen: CDLA-Permissive-2.0 von webpki-roots neben GPL-3.0 (O2), CLA 1.0 ohne
   anwaltliche Prüfung (G6), Kopplung von `dictate-ui` und niri-ipc (Abschnitt 2 des Plans). Eine Download-Seite gibt es
   erst mit Releases und Builds (V20); bis dahin sind die GitHub-Releases mit Quelltext-Tarball der Download-Ort. Der
   nächste Release folgt „Release und AUR“ unter „Fallen“.
2. **Beobachten** (aus der Live-Abnahme, Liste in `PLAN-wayland.md` 6h): mehrere Monitore, fullscreen/maximized,
   Sperre im Abgang und Weg „Frame gemeldet“ (nur Unit-Test), Hinweis nach echtem unsauberem Boot-Ende, Schwelle
   `CONFIDENT_*` (`src/palette/resolve.rs`) nach einigen Wochen, `ASK_FRAME` live, Diagnose-Messung mit Symptom und
   mit 33 Proben (`live_eval.rs`; Fall 2 prüft seit W7 die letzte Zeile des temp-loggers statt des Xorg-Logs),
   Prompt-Punkte (t16 knapp über 3×, „Dear Sam“ → „Lieber Sam“), Wackeln von `palette.sh` und `T3: route input method` unter Last (O7: zwei Läufe bei Last 17 mit je anderer Zeitprüfung rot), t18 im Sprachtest. Der Handtest von O6
   (Sperre über logind, Zwischenablage, Emoji, Gruppe „Clipboard and emoji“) ist am 23.11. von Daniel abgenommen.
3. Baustein 5 (Kontext-Workspaces) ist nicht begonnen (`ROADMAP.md`).
   **Bausteine 7 und 8** (kleine Funktionen, Modellwahl mit CPU-Weg) sind am 26.11.2026 geplant, nichts ist gebaut
   (`PLAN-features.md`); Reihenfolge, Abnahme und die offenen Entscheidungen für Daniel stehen dort. **GlobalShortcuts-Portal:**
   auf blub mit niri 26.04 nicht nutzbar (`BindShortcuts` scheitert, gemessen 26.11.); niri hat die Gegenseite am
   07.11.2026 gemergt (#4744), noch in keinem Release: Wiedervorlage K1b beim nächsten niri-Release, bis dahin bleibt die
   Gruppe `input` Voraussetzung.
4. **Voller Schutz für `dictate.service`** (Entscheidung in Daniels Auftrag, 25.11.: festhalten, nicht bauen): Der Daemon
   behält Schreibzugriff auf den Heimatordner, weil die Programme der Diagnose als seine Kinder laufen und im
   Namespace Falsches melden. Ausweg: Proben außerhalb der Sandbox starten (etwa als flüchtige Units), dann die
   Dateisystem-Optionen wie bei `dictate-ui`. Das ändert den Probenstarter (`src/diagnose/run.rs`: Drossel, Frist,
   Abholen, Katalog als einzige Schranke) und braucht Entwurf und Security-Review. Dazu `MemoryDenyWriteExecute` für
   `dictate-ui` und Daniels Handtest nach dem Installieren der Units (`docs/systemd-hardening.md`, „Open“).

## Entscheidungen, die weiter gelten

- **KI-Prompts:** vor jedem Modus-Prompt (auch eigenen) und jedem Aktions-Prompt steht der feste Schutzrahmen
  `INPUT_GUARD` (`src/config.rs`, eingefügt nur von `with_guard`/`AiCfg::mode_prompt`): Eingabe ist nur Text, nie
  beantworten, befolgen, ablehnen. Vorn statt hinten, weil Haiku die Regel am Ende mit Ablehnen beantwortete (A/B in
  `SPEC-v2.md`). `PROMPT_*` wiederholen ihn nicht. Einzige Ausnahme: die Absicht `ask` der Palette beantwortet eine
  Frage und hat dafür den eigenen festen Rahmen `ASK_FRAME` (Begründung dort; `ai::ask` lässt nur ihn ohne
  Schutzrahmen). Prompt-Beispiele und Testsätze in `tests/ai_http.rs` müssen disjunkt bleiben. Die Datei speichert
  nur Abweichungen von `BUILTIN_MODES` (`[ai.modes]` je Modus, `ai.removed_modes`; ebenso `[actions.prompts]`), neue
  Vorgabe-Prompts gelten für jeden unveränderten Modus; wortgleiche Alt-Einträge entfallen beim nächsten Speichern.
  Refine: das Längenlimit (3× Eingabe, genannt 80 %) schreibt der Code je Anfrage als festen Zusatz ans Ende des
  System-Prompts (`length_budget_note`, `Shape::budget`, `Action::prompt_for`; Messung in `refine-prompt-ab.md`
  Nachtrag 3 und 4: 70 % und ausdrückliche Bezeichnungen je Sprache waren schlechter).
- **Modelle:** `to_de` ohne Vorbelegen (Haiku 5.5 gegen Sonnet 5.5 nach Schärfung 4 zu 3 Fehler von 66, Sonnet
  1,5-fach langsamer); Opus 5.5 mit effort low live ohne zweiten Versuch. `diagnose.model` bleibt Sonnet 5.5 (Daniel
  21.11.; Haiku 5.5 mit strict traf ebenfalls 8 von 8, spekuliert aber mehr; `diagnose-ai.md`). `palette.model` folgt
  beim Anbieterwechsel wie die übrigen Modelle (`choose_ai_preset`). Ollama lokal nicht einrichten (Daniel 21.11.:
  Qualität und VRAM neben Whisper).
- **Fehler der KI** zeigen Panels nur als Code und feste Meldung (`AiError::client_text`, auch `AiError::Stream` mitten
  im Strom), nie mit Servertext (Daniel 21.11., wie `pal_ask`).
- **Claude-Schlüssel:** `~/.config/anthropic/api-key` (0600) war am 21.11. ungültig (HTTP 401) und ist durch den
  gültigen aus `console-api-key` ersetzt (beide gleich, Daniel 21.11.).
- **pstore** ist auf dem Rechner des Autors für den eigenen Benutzer lesbar: `/etc/tmpfiles.d/pstore-blub.conf`
  (Lese-ACL, auch Vorgabe-ACL) und Drop-in `/etc/systemd/system/systemd-pstore.service.d/acl-blub.conf`
  (`ExecStartPost` setzt sie nach dem Archivieren, weil `systemd-pstore` mit 0600 anlegt). `suspend` ist dort per `sleep.conf` gesperrt.
- **Diagnose-Symptom** („What is wrong?“): eigener gekennzeichneter Teil der Anfrage, durch den Filter, in der
  Vorschau, nie geloggt.

## Bauen und installieren

```bash
scripts/check.sh --list        # Prüfschritte: fmt licenses clippy unit wayland gui speech gpu (billig zuerst)
scripts/check.sh --only fmt,clippy,unit   # nur betroffene Schritte; ohne --only alles, Abbruch beim ersten Fehler
cargo build --release          # Toolchain 1.95.0 über rust-toolchain.toml (eframe 0.37); Default-Toolchain 1.91 bleibt
cargo install --path . --locked                       # installiert dictate, dictate-settings und dictate-ui nach ~/.cargo/bin
sudo systemctl --user -M blub@.host restart dictate dictate-ui   # aus einer Claude-Shell geht systemctl --user nur so
```

`niri/dictate-binds.kdl` kommt über chezmoi nach `~/.config/niri/` (nur mit Daniels Zustimmung, nur zusammen mit
passenden Binaries: Fensterregeln „dictate: leaving“ und Ecken von `dictate-settings` gehören zum Code). Prüfen ohne die echte
Sitzung: `niri validate -c niri/dictate-binds.kdl`.
In zsh Befehle mit Umgebungsvariablen nicht in einer Variable speichern (keine Wortaufteilung), sondern als Funktion.

**Testordner `DICTATE_TEST_ROOT`** (seit O7, `scripts/test-root.sh`, in Rust `tests/test_root/mod.rs`, Wächter
`tests/wayland/nested/src/guard.rs`): Logs, Sperre von `check.sh`, Laufzeitordner des verschachtelten niri (`wl-run/`)
und Testaudio liegen darunter. Vorgabe `${XDG_CACHE_HOME:-$HOME/.cache}/dictate-test`; nur absolut, ohne `.`/`..`,
Zeichen `A-Z a-z 0-9 . _ - /`, höchstens 55 Zeichen (Unix-Sockets der verschachtelten Sitzung). Auf dem Rechner des Autors gehören Logs
und Testaudio nach `/home/data/stt-test`: Daniel setzt die Variable in `~/.zshenv` (über chezmoi); Agenten setzen sie nicht in
Dotfiles, sondern im Befehl, etwa `DICTATE_TEST_ROOT=/home/data/stt-test scripts/check.sh --only fmt,clippy,unit`.
Zwei Ordner heißen zwei Sperren: ein Lauf ohne Variable sperrt einen Lauf mit Variable nicht (dann nie zwei
gleichzeitig starten, Hitze). `check.sh` nennt den Ordner in der ersten Zeile; ohne Aufnahmen im Ordner wird `speech`
übersprungen, `gpu` scheitert. Die Spikes unter `spikes/` prüfen noch den festen Ordner `/home/data/stt-test/wl-run`.

## Arbeitsweise mit Agenten

Ergänzt `~/.claude/CLAUDE.md`. **`scripts/check.sh` ist die einzige Prüfquelle** (Schritte und Logs im Kopf des
Skripts; Logs unter `$DICTATE_TEST_ROOT/logs/check/latest/`, auf dem Rechner des Autors `/home/data/stt-test/…`, im Terminal nur
Ergebnis und Zeit). Agenten lassen nur
die Schritte laufen, die ihre Änderung berührt; die volle Suite (ca. 25 min, gemessen 22.11.: unit 7–10, wayland 8–11, speech 4, gui 2, gpu 2) läuft einmal am Ende in der Hauptsitzung.

| Änderung | Schritte |
|---|---|
| Rust-Logik (Daemon, Pipeline, Protokoll, Aktionen) | `fmt,clippy,unit` |
| Abhängigkeiten (`Cargo.toml`, `Cargo.lock`, Features) oder Toolchain; Drittlizenzen (`licenses/`, `deny.toml`) | vorher `scripts/third-party-licenses.py`, dann `fmt,licenses,clippy,unit` (bei `src/gui` oder egui/eframe dazu `gui` und `--bin dictate-settings`) |
| Einstellungsfenster (`src/gui/`: Seiten, Thema, Schrift, Beobachter, Logo) | `fmt,clippy,gui`, dazu `cargo test --release --bin dictate-settings` (Kontrast, Schrift, `watch.rs`, Logo, Vorschau; `gui` baut nur und klickt) |
| Gemeinsame Teile von Overlay und Einstellungsfenster (`src/ui/wave.rs`, `src/ui/palette.rs`, `src/ui/fsutil.rs`) | `fmt,clippy,gui,wayland`, dazu `--bin dictate-settings`, `--bin dictate-ui`, `--lib ui::` |
| Audio, Engine, Streaming, Pipeline-Text | `fmt,clippy,unit,speech` (`gpu` bei Engine/Streaming/Audio) |
| Diagnose (Rust: Katalog, Proben, Filter, KI, Dienst) | `fmt,clippy,unit` |
| Units (`systemd/*.service`) und alles, was `dictate` oder `dictate-ui` als Kindprozess startet (Katalog der Diagnose, Kurzläufer der Palette) | `tests/systemd/probe.py dictate`, `tests/systemd/probe.py dictate-ui`, `tests/systemd/programs.py` (je unter 1 min, lesend, nicht in `check.sh`: sie brauchen den User-Manager), `systemd-analyze --user security --offline=yes systemd/<unit>.service`; bei `dictate-ui.service` dazu `DICTATE_UI_UNDER_UNIT=1 tests/wayland/run.sh`, bei `dictate.service` die GPU-Tests über `tests/systemd/under-unit.py dictate -- <Test-Binary> --ignored …` |
| Wayland-Oberfläche und Textausgabe (`src/ui/`, `src/bin/dictate-ui/`, `niri/dictate-binds.kdl`; die KDL auch `gui`, dort prüft `tests/gui/lib.sh` das Schweben von `dictate-settings`). `wayland` = `tests/wayland/run.sh` mit `output.sh`, `actions.sh`, `diagnose.sh` (Diagnose-Panel, Boot-Hinweis), `palette.sh` (Befehlspalette gegen Mock und Fake-Programme, P1–P27), `lock.sh`, `logout.sh` (P28, beendet das verschachtelte niri, darum zuletzt) | `fmt,clippy,unit,wayland` (gezielt statt `unit`: `cargo test --release --bin dictate-ui` (2 s) **und** `--lib ui::…`, bei `src/ui/pal_*`/`cmdpal*` auch `--lib palette::ui_mirror` und `--lib ui::cmdpal`; nur `--lib` ließ `sources_stay_apart` vier Commits lang rot) |

- **Testsatz:** `tests/speech/testset.tsv`, Aufnahmen des Users unter `$DICTATE_TEST_ROOT/audio/testset/<id>.wav`
  (nur er nimmt auf: `scripts/record-testset.sh` braucht ein Terminal). Schwellen und harte Prüfungen in
  `tests/speech/thresholds.tsv`, nur nach absichtlicher Änderung neu ableiten (Regel steht dort), nie zum Grünmachen.
  Ergebnis je Satz: `logs/check/latest/speech.tsv`. Stand 23.10.: `speech` grün (t11 behoben: Satzzeichen von Whisper
  vor einem Umbruchbefehl bleiben, siehe Kopf von `commands.rs`). 21.11.: t18 (Absatz) wackelt, Whisper ließ in 2 von 3
  Läufen einen Halbsatz aus (CER 0,104/0,094, max 0,09), der nächste Lauf 0,0; Schwelle nicht angefasst (`PLAN-wayland.md`
  6e, Abschluss).
- **Textausgabe unter Wayland** (`dictate-ui`, W2): Reihenfolge Eingabemethode (T3) → Einfügen (nicht in Terminals,
  nicht bei unbekannter `app_id`) → eigene virtuelle Tastatur (T2). Zeichen von T2 nur auf den Codes in
  `ui::keymap::CHAR_CODES` (Chromium deutet z. B. Code 14 als Rücktaste), jede Taste in einem `flush` gedrückt und
  losgelassen (niri lässt beim Zerstören nicht los). Trägt die Zwischenablage `x-kde-passwordManagerHint: secret`,
  wird getippt und nur der Hinweis gelesen. Eingabemethode nur für die Dauer einer Ausgabe (eine dauerhafte legte
  fcitx5/ibus lahm); niri schickt ihr `activate` erst nach einer Rundreise. Inhalte und Text nie loggen.
- **Wayland-Tests** nur im verschachtelten niri (`tests/wayland/nested/harness.sh`: kopfloses sway, eigener
  Laufzeitordner unter `$DICTATE_TEST_ROOT/wl-run`), nie gegen die echte Sitzung, nie `niri msg` an das echte niri,
  keine uinput-Geräte; `dictate-ui` liest den Tastenzustand dort aus einer Datei (`--fake-keys`). `src/ui/keys.rs`
  darf Eingabegeräte nur mit `ioctl` (`EVIOCGKEY`, `EVIOCGBIT`) abfragen, nie lesen (Test im Modul). Der System-Bus ist
  in jedem Wayland-Test ein privater `dbus-daemon` im Laufzeitordner (`DBUS_SYSTEM_BUS_ADDRESS`, `XDG_SESSION_ID`
  entfernt; `run.sh` bricht ohne ihn ab), darauf ein falsches logind (`tests/wayland/fake-logind`) für
  `tests/wayland/lock.sh`; nie das echte logind. Klickziele im Panel kommen aus dem Debug-Log `panel: layout <id> …`.
- **GUI-Tests** (`tests/gui/`, Einstellungsfenster gegen `tests/gui/mock_daemon.py`) laufen ebenfalls im verschachtelten
  niri, gemeinsame Teile in `tests/gui/lib.sh`: Ausgabe 2600x1950 bei 1,625 (erst ab dieser Höhe öffnet niri das
  schwebende Fenster in voller Größe 1180x860; `gui_start` prüft Größe und Schweben, bevor geklickt wird), Belegung
  `us` (der Injektor drückt evdev-Codes an US-Positionen), eine Raste des Mausrads 40 Pixel. Klickziele seit W6
  Block B aus dem Lage-Log des Fensters (`src/gui/layout_log.rs`, nur mit `DICTATE_DEBUG`, das `gui_start` setzt;
  `gclick_id`, `gui_reveal`, `gui_spot` in `lib.sh`), nie feste Zahlen; ein neues Ziel braucht ein
  `layout_log::note` mit fester ID (nie aus Inhalten). Fixtures unter `tests/gui/fixtures/` (Schrift über
  `gtk-4.0/settings.ini`, nach Messpunkt M1 die einzige Quelle von GTK 4 unter niri; Palette dunkel/hell als
  `DICTATE_PALETTE`, `GUI_PALETTE=light`), nie Daniels Einstellungen. Ein Skript hält Harness, Mock und Fenster selbst
  (trap). Manuell: `tests/gui/shots.sh BINARY [PREFIX]` (alle Seiten), `tests/gui/interact.sh` (nicht in `check.sh`).
- **dictate-ui und GTK/niri:** einen abgelaufenen glib-Zeitgeber (`timeout_add_local_once`, Rückgabe `Break`) nie mit
  `SourceId::remove` entfernen (Panic): im Rückruf das gemerkte `SourceId` nur vergessen. `glib::set_prgname` vor
  `gtk::init` setzt die `app_id` der Fenster (`dictate-panel`, Fensterregel in `niri/dictate-binds.kdl`). Schreibende
  niri-Aktionen (`set_window_height`, `move_window_by`) nur auf das eigene Fenster (`niri::own_window`/`placement`:
  `app_id` **und** eigene PID, Entscheidung 13). Ragt das Panel nach dem Wachsen unten hinaus, schiebt es der Resizer
  relativ hoch (`niri::shift_into_view`; niri behält die Oberkante und klemmt nicht, `SetFixed` zählt im
  Arbeitsbereich, den IPC nicht nennt). Hover im Panel baut nichts neu auf (`Effect::Highlight`), sonst verschluckt der Neuaufbau Klicks.
  Die Sperre (`lock.rs`) läuft über gio im Hauptthread, Abos nur mit Absender `org.freedesktop.login1`, vor
  `GetSession`; `exit-on-close` aus (sonst endete `dictate-ui` mit 0, ohne Neustart), Neuverbindung mit Backoff.
  Beobachtet werden die Sitzungen aus `XDG_SESSION_ID` **und** `auto` (gelöscht wird bei jeder), das Öffnen sperrt
  nur der `LockedHint` der Sitzung hinter `auto` (auf dem Rechner des Autors war die Wayland-Sitzung am 20.11. Nummer 2, im Spike 4).
  Sperre: Panel zu, `act_clear`/`diag_clear` auf eigenen Verbindungen (`Act::clear_all_now`), Ausgabe abgebrochen
  wie Mod+Ctrl+E, Mod+I verweigert (Hinweis im Overlay).
- **dictate-ui, Regeln aus der Durchsicht W3:** Befehle an den Ausgabe-Thread mit Frist nur über den Anspruchs-Slot
  (`output::Claim`: aufgegeben = nie mehr ausgeführt; ein „gescheitertes“ Copy setzte sonst später doch CLIPBOARD).
  `panel_visible` (Hauptthread setzt es bei `Show`/`Hide`) sperrt jede Ausgabe ins Panel, auch je Zeichen, unabhängig
  von niri. Jedes neue Modul in `src/bin/dictate-ui/` gehört in die Liste von `sources_stay_apart` (`primary.rs`).
  Markdown aus KI-Antworten: Schachtelung höchstens 8, Suchen gemerkt (`Finder`), Test mit langen Markenketten.
- **dictate-ui, Regeln aus der Durchsicht W4:** Das Feld „What is wrong?“ nur mit `grab_focus_without_selecting`
  fokussieren (sonst markiert GTK den Inhalt und bietet das ungefilterte Symptom als PRIMARY an; Prüfung über
  `nested-clip watch-primary`), `InputHints::PRIVATE`. Das Diagnose-Panel schließt mit Ablauf der Daemon-Sitzung
  (`expires_in_s`, auch bei aktiver Nutzung) und löscht Bericht und Antwort, sobald ein Aufruf die Sitzung als
  unbekannt meldet (`CallErr::unknown_session`, etwa nach Neustart des Daemons).
  Klickziele im Panel erst berechnen, wenn es liegt und ruht (`panel_settled` in `tests/wayland/actions.sh`: das
  Hochschieben läuft im Resizer-Thread). Neue Zeilen oder längere Texte auf einer Seite des Einstellungsfensters
  prüft der Schritt `gui`; seit W6 Block B kommen die Klickziele dort aus dem Lage-Log, nicht aus festen Zahlen.
- **dictate-ui, Regeln aus W5 (Befehlspalette):** Neue Module in `src/bin/dictate-ui/` in `MODULES`
  (`sources_stay_apart`). `no_shell_anywhere` erlaubt genau ein `Command::new(exe.path())` (`pal_host.rs`) und genau ein
  `Action::Spawn` (`niri.rs`), kein `SpawnSh`; `src/ui/*.rs` startet keinen Prozess; Programme nur über die Naht
  `Programs` (Suchpfad, Tests legen ihn auf Fake-Programme, `run.sh` bricht ohne sie ab). Jede niri-Aktion nur mit
  der ID aus dem Kandidaten bzw. dem Fenster vor dem Öffnen, nie „das fokussierte“. Nie `{:?}` auf Palettentypen;
  `CallErr` und `pal::Origin` ohne `Debug`. Jeder Auftrag an eine Bahn in `pal.rs` antwortet genau einmal. In `app.rs`
  nie `match this.borrow()…` mit `borrow_mut` im Rumpf. Eingabe, Kontext, Kandidaten, Ziel-IDs, Antworten,
  CLIPBOARD, Frage und Suchbegriffe nie loggen (Log-Prüfung in `run.sh` mit den Wörtern `qq…` des Mocks); auch die
  gesprochene Eingabe nicht mit `DICTATE_DEBUG`: der Daemon reicht `log_text` (nur Fensterdiktat) über `Work::Begin` an
  Streamer und `pipeline::process` (Test `tests/daemon_voice_log.rs`, eigenes Binary). Aus der Durchsicht
  (Entscheidung 18): `text_action` aus der Palette zeigt die Vorschau und sendet erst auf Return (wie
  `explain_clipboard`); eine offene Bestätigung nimmt nur Return oder „Confirm“, kein zweiter Klick auf die Zeile.
  **Abgang vor Screenshots** (`full`/`area`, Entscheidung 17): Deckkraft 1/255 (bei 0 übergibt GTK 4.24 keinen
  Frame, niri behält den deckenden) und Titel „dictate: leaving“; beide stellt nur `show` vor `present` zurück, nie
  `hide` (ein deckender Frame vor dem Verbergen wäre wieder im Bild). Der Titel und die Fensterregel „dictate: leaving“
  in `niri/dictate-binds.kdl` gehören zusammen (ohne Regel füllt niri die Fläche in Ringfarbe, Ring und Schatten im
  Bild); darum `dictate-ui` und KDL nur zusammen installieren. Im Abgang wirkt keine Eingabe (L39, Entscheidung 19: auch Esc nicht; GTK 4 stellt
  Tasten erst mit dem nächsten Takt der Frame-Uhr zu, die bis zum Frame-Rückruf von niri steht, ein Esc käme ohnehin
  zu spät).
- **Exklusive Ressourcen:** je Testlauf ein eigener Laufzeitordner des verschachtelten niri (`wl-run/n.*`), eigene
  Sockets und Konfigurationen je Test-Daemon, Pulse-Nullsenken `dictate_e2e_*`/`dictate_check_*`; GPU- und
  Pulse-Schritte nie parallel. `check.sh` sperrt sich selbst (`logs/check/.lock`) und räumt auch bei Abbruch auf
  (Prozessgruppe, eigene Pulse-Module; das verschachtelte niri baut das Testskript per trap ab). Wer Teile von Hand
  startet, beendet sie selbst (`pactl unload-module`; ein abgebrochenes Testskript hinterlässt nichts, sonst
  „teardown left processes“).

## Fallen

- **Ohne Silero-VAD erfindet Whisper in Pausen „Vielen Dank.“** VAD nie abschalten.
- **Silero-VAD ist pegelabhängig**: leise Mikrofone (Brio, Sprache um −40 dBFS) nur mit Eingangsverstärkung
  (`src/gain.rs`, `max_gain_db`, Standard 30). Finaler Durchlauf: Schnittkanten weich (`edge_fade_ms`), Pausen unter
  `span_join_ms` bleiben; harte Schnitte kippten Whisper („Test 1, 2, 3“ → „Test“). E2E: `tests/daemon_pulse_real.rs`.
- CUDA-Erstbau dauert ~10 min, nie `cargo clean`. Keine CUDA-Einstellungen im Repo (seit O3): whisper.cpp baut mit
  `GGML_NATIVE` für die eingebaute GPU (auf dem Rechner des Autors 86, gemessen per CMake-Configure), nvcc über `PATH`/`CUDA_PATH` aus
  `/etc/profile.d/cuda.sh`. `build.rs` von whisper-rs-sys beobachtet `CMAKE_*`/`CUDA*` nicht: eine Änderung wirkt
  erst nach `cargo clean -p whisper-rs-sys` (nur mit Daniels Zustimmung, 10 min Bauzeit).
- Die echte niri-Sitzung, `dictate.service` und `dictate-ui.service` nur bewusst anfassen. Tests laufen im
  verschachtelten niri bzw. mit eigenem Socket (`DICTATE_SOCKET`) und eigener Config (`--config`). Prozesse nur per PID
  beenden, nie `pkill -f`.
- Anthropic: kein `temperature` senden. Denk-Parameter je Modell (`Thinking` in `src/pipeline/ai.rs`, Tabelle in
  `SPEC-v2.md`): Sonnet 5.5 `between_tools` (`disabled` = HTTP 400), Opus 5.5/Fable nicht abschaltbar (`effort: low`,
  mehr `max_tokens`), Haiku 5.5/Opus 5/Sonnet 5 `disabled`, 4.x und älter nichts; unbekannte Modelle lernen aus der
  400-Meldung. Neue Modelle in die Tabelle eintragen (Skill `claude-api`). Der Schlüssel des Users liegt in
  `~/.config/anthropic/api-key` (0600), dictate liest ihn nur über `ai.api_key_file`. Schlüssel nie ausgeben, nie in Logs oder die Config.
- Config `~/.config/dictate/config.toml` existiert (vom Fenster geschrieben); Agents ändern sie nie selbst. Schreiben
  nur über `Config::save` (toml_edit, Kommentare bleiben, 0600). Im Speicher und im JSON stehen alle wirksamen
  Modi/Prompts, in der Datei nur Abweichungen (`Config::from_toml`/`render_toml`). `ai.model` leer = Vorgabemodell des Anbieters (`AiCfg::effective_model`);
  Modellprüfung nur beim Speichern (`Config::validate_new`), nie beim Laden.
- **Design: Noctalia-Palette statt Pushing Pixels** (Entscheidung Daniel, 20.11.2026, für die ganze Anwendung): Farben
  aus den Material-Rollen, die Noctalia über die Vorlage `noctalia/dictate.json.tmpl` nach
  `~/.local/state/dictate/palette.json` schreibt (`src/ui/palette.rs`, Rückfall Tokyo Night dunkel, Datei wird
  beobachtet; die Häufigkeit der Palette liegt daneben in `frecency.json`, nie unter diesem Namen; eine alte
  Häufigkeit in `palette.json` holt der Daemon nur nach Inhalt ab (`frecency::adopt_legacy`, „Clear“ löscht sie mit
  `frecency::remove_legacy`) und leert sie nur über den eigenen Deskriptor (`Legacy::empty`), nie `unlink` per Namen), Schrift aus
  `gtk-font-name` in `settings.ini`, Ecken 12/8/4 (`RADIUS`, `RADIUS_CONTROL`, `RADIUS_CHIP`). Umgesetzt in
  `dictate-ui` und seit W6 im Einstellungsfenster (`src/gui/theme.rs`, Farben nur über `theme::th()`); Regeln in
  `DESIGN.md` (seit W6 Block E neu gefasst; Pushing-Pixels-Guide, `design/pushingpixels/`, `assets/brand/` und die PNGs
  unter `src/gui/assets/` sind gelöscht, nur noch in der Git-Geschichte). Neue Textpaarung im Fenster: in
  `Theme::text_pairs` eintragen (Kontrasttest). `assets/logo/` bleibt: `icon-512.png` ist das Fenster-Icon (nach
  Änderung an `icon.svg` neu rendern), `glyph-on-dark.svg`/`lockup-on-dark.svg` sind die Vorlage des aus Rollen
  gezeichneten Logos (Test `logo_follows_the_svg`). Sichtbare Texte weiter nur Englisch.
- **espanso liest Tasten über evdev, vor niri:** eine Bindung in `niri/dictate-binds.kdl` verhindert nicht, dass
  espanso auf dieselbe Taste reagiert. Am 23.11. löste `Mod+Alt+Space` zugleich die Suche von espanso aus; abgeschaltet
  mit `search_shortcut: off` in `~/.config/espanso/config/default.yml` (Daniels Wahl, nicht über chezmoi). Neue Kürzel
  vorher gegen espanso (`toggle_key`, `search_shortcut`) und niri-coach (`coach-binds*.kdl`) prüfen.
- **Synthetische Tasten:** unter Wayland drückt und löst `dictate-ui` jede Taste der eigenen virtuellen Tastatur in
  einem `flush` (siehe „Textausgabe unter Wayland“); ein Abbruch (Mod+Ctrl+E) lässt nichts gedrückt. Die X11-Regeln zu
  `xdotool`, `dictate release-keys` und `dictate synth` entfielen mit W7 (Herleitung in der Git-Geschichte vor
  `remove-awesome`).
- **Härtung der Units** (`docs/systemd-hardening.md`): Kindprozesse erben die Sandbox ihrer Unit. (1) Ein Fenster nie
  als Kind von `dictate-ui` starten, nur über `niri::spawn_at` (das Einstellungsfenster: `pal_exec::settings_launch`,
  auch für „Settings“ in den Panels); ein Kind hätte einen nur lesbaren Heimatordner und kein Netz. (2) Ein neues
  Programm im Katalog der Diagnose oder ein neuer Kurzläufer der Palette kommt in `tests/systemd/programs.py` und muss
  dort ohne und unter der Unit gleich antworten (so fielen `ip`/`ping` am Filter und `findmnt`/`lsblk`/`ss` im
  Namespace auf). Das Skript startet die Programme direkt: was der Daemon um eine Probe herum tut, zeigen nur seine
  Tests unter der Unit (`under-unit.py dictate -- <daemon_diagnose-Binary>`; so fiel auf, dass der Filter
  `setpriority`/`ioprio_set` der Drossel sperrte und keine Probe mehr startete). Ein neuer Systemaufruf im
  Probenstarter oder im Daemon braucht darum diesen Lauf. (3) Darum bekommt `dictate.service` keine Option, die einen Namespace braucht (im User-Manager auch
  `ProtectClock`, `ProtectHostname`, `CapabilityBoundingSet`): nicht „nachziehen“, solange die Diagnose ihre Programme
  als Kinder startet. (4) `dictate-ui` schreibt nur in Zustands-, Cache- und Laufzeitordner (`StateDirectory`,
  `CacheDirectory`, `RuntimeDirectory`, alle `dictate`); ein neuer Schreibpfad im Overlay braucht eine Zeile in der
  Unit. (5) Flüchtige Test-Units nie mit den Verzeichnis-Direktiven unter dem Namen `dictate` (systemd fasst sonst
  die Ordner der laufenden Dienste an) und nie mit der Umgebung des User-Managers (das ist die echte Sitzung):
  `tests/systemd/unit.py` regelt beides. Kein `MemoryDenyWriteExecute` für `dictate-ui` (NVIDIA-OpenGL-Treiber).
- **Lizenzköpfe** (seit U1, `PLAN-lizenz.md`): jede neue `.rs`-Datei beginnt mit dem Kopf aus `RS_HEADER` in
  `tests/license_headers.rs` (byte-genau, dann Leerzeile), jedes neue Shell- oder Python-Skript trägt nach dem Shebang
  den Kurzkopf `SCRIPT_HEADER`; der Test im Schritt `unit` findet fehlende.
- **Drittlizenzen** (seit U2, `PLAN-lizenz.md`): `THIRD-PARTY-LICENSES.txt` und `licenses/rust-std-*.html` sind
  erzeugt, nie von Hand ändern; nach jeder Änderung an `Cargo.lock`, Features oder `rust-toolchain.toml`
  `scripts/third-party-licenses.py` laufen lassen und das Ergebnis mitcommitten (Schritt `licenses` prüft es und mit
  `cargo deny` die Erlaubnisliste `deny.toml`). Ein neues Paket ohne Lizenzdatei bricht das Skript ab: Datei aus dem
  Upstream am Commit aus `.cargo_vcs_info.json` nach `licenses/upstream/` legen, `UPSTREAM` und `SOURCES.md`
  ergänzen. Eine neue Lizenz in `deny.toml` nur mit Beleg der GPL-Kompatibilität (Abschnitt 2 des Plans).
  `default_fonts` von egui/eframe bleibt aus (Daniel, 23.11.: keine fremde Schrift im GPL-Binary; `deny.toml` lässt
  OFL und Ubuntu Font Licence absichtlich nicht zu); Schriften nur vom System (`gui::fonts`). Das gilt auch für den
  Dependabot-PR (cargo, monatlich): auf seinem Branch neu erzeugen und mitcommitten.
- **Das Repo ist öffentlich** (seit 24.11.2026, U5): Alles darin, auch die Pläne und diese Datei, steht im Netz. Keine
  Geheimnisse, fremden Namen, Kennungen oder geschäftlichen Angaben (Preise, Zahlungsanbieter, Steuern) hineinschreiben;
  Geschäftliches gehört in die private Datei außerhalb des Repos. `README.md` ist nur englisch (Daniel, F4). Vor einem
  Push, der nach außen wirkt, wird der Baum mit `git archive` exportiert und mit gitleaks und dem Prüfskript der U5a
  gelesen (das Skript enthält Suchmuster für Geschäftliches und liegt darum privat gesichert, nicht im Repo; es
  gibt nie einen gefundenen Wert aus). Secret Scanning mit Push Protection und Private Reporting sind auf GitHub an.
  Die unversionierten Ordner `reports/` und `research_notes/` im Archiv-Checkout (`/home/git/blub/dictate-archiv`)
  sind Daniels private Recherche: nicht anfassen, nie committen.
- **Release und AUR** (U5 Schritt 6, `packaging/aur/`): Eine neue Fassung ist ein neuer Tag; Tag, GitHub-Release und
  AUR-Upload nur mit Daniels Freigabe je Schritt. Danach `updpkgsums`, `.SRCINFO` nur aus `makepkg --printsrcinfo`, im
  Arbeitsklon außerhalb des Repos (`/home/git/blub/aur-dictate`, AUR-Konto `derblub`) committen und pushen. Der
  Bau-Test des PKGBUILD im Container `archlinux:latest` (Skript privat gesichert) belegt mit allen
  Standardarchitekturen den Host über eine Stunde: Wiederholungen mit `CMAKE_CUDA_ARCHITECTURES=86` und unter dem
  gemeinsamen Host-Lock (Datei in `~/.cache`, Vorgabe der Sitzungen).
- Mehrere Agents im selben Repo: nur eigene Pfade stagen, nie `git add -A`. Keine Attributionszeilen in Commits.
- Große Logs nach `$DICTATE_TEST_ROOT/logs/`, Testaudio unter `$DICTATE_TEST_ROOT/audio/` (von Tests benutzt; auf
  dem Rechner des Autors `/home/data/stt-test`).
- **Hitze**: Builds mit `CARGO_BUILD_JOBS=4`, nie zwei cargo-Läufe parallel, gezielte Testfilter, ganze Suite einmal am
  Ende. GUI-Tests nur gegen den Mock im verschachtelten niri (`tests/gui/lib.sh`, `MOCK_CLOUD=1` für Cloud-Hinweise),
  Screenshots per `tests/gui/shots.sh`; sway, niri und Mock enden mit dem Skript, nichts darf weiterlaufen.
- Textaktionen: Markierung und Antworten nie loggen (auch nicht mit `DICTATE_DEBUG`), nie in `history.rs`. Nur PRIMARY,
  nie CLIPBOARD lesen. Zwei Ausnahmen: (1) das Einfügen von `dictate-ui` (`src/bin/dictate-ui/clip.rs`) sichert
  CLIPBOARD, um sie nach dem Einfügen unverändert zurückzulegen; Inhalt nur im Speicher dieses Prozesses, nie loggen,
  nie an Daemon oder KI (trägt sie `x-kde-passwordManagerHint: secret`, wird nur der Hinweis gelesen und getippt).
  (2) Absicht `explain_clipboard` der Befehlspalette: CLIPBOARD wird nur auf diese ausdrückliche Absicht gelesen, mit
  gleicher Frist und Grenze wie PRIMARY (`actions::selection`); bis W7 tat das `dictate act grab --source clipboard`,
  unter niri seit W5 nur `clip::read_clipboard_text` ← `Cmd::ReadClipboard` ← `OutTx::read_clipboard` ← `act::grab`
  (einziger Weg, Hinweis `secret` = leer). Der Daemon prüft Geheimnisse (Treffer + Cloud = gesperrt mit `describe`),
  speichert die Sitzung und gibt nur die Vorschau zurück; gesendet wird erst mit `act_run` nach Bestätigung im Panel.
  Inhalt nie loggen, nie in `history.rs`, nie ohne Vorschau senden. Keine Shell: ausgeführt wird nur per argv, nie über
  `sh -c` (bis W7 prüfte der Xephyr-Test das für Lua: kein `with_shell`/`io.popen`/`os.execute`).
- Geheimnisfilter: `assets/wordlists/eff_large_wordlist.txt` ist die unveränderte EFF-Liste (Test prüft die Prüfsumme,
  Lizenz CC BY 4.0 seit O1, Hinweis in README); nicht bearbeiten. Neue Arten in `secrets::KINDS` brauchen `describe` (GUI und Panel).
- **Diagnose-Bericht, Ausgaben der Proben und Antworten nie loggen** (auch nicht mit `DICTATE_DEBUG`) und nie in
  `history.rs`; im Log nur Probe, Dauer, Bytes, Zahl der Schwärzungen, Modell und Tokens (Test
  `reports_and_probe_output_never_reach_the_log`). Der Klartext der Pseudonyme erscheint nie im Log, in Copy report oder in
  Aufrufen an den Daemon. Bericht, Ergebnisse und Antworten leben nur im Speicher der Sitzung (höchstens 30 min).
- **Proben in Tests nur gegen Fakes und synthetische Fixtures**, nie gegen das echte System: `ProbeContext::system` und
  `ProbePaths::system` gibt es unter `cfg(test)` nicht (Tests nehmen `ProbePaths::rooted`, `ProbeEnv::with_bin_dir` und
  Fake-Programme in Temp-Ordnern). Keine echten Logs des Rechners des Autors im Repo, auch nicht gekürzt; der Korpus der Fehlalarm-Messung
  entsteht aus `tests/diagnose/corpus/generate.py`.
- **Der Katalog ist die einzige Schranke, keine freien Befehle.** Eine neue Probe ist ein neuer Eintrag in `src/diagnose/`
  (argv-Vorlage als Daten, typisierte Parameter, Grenze, Frist, Begründung „Read-only, weil …“ am Enum; die Tests prüfen
  verbotene Wörter und eine Erlaubnisliste je Programm). Nie `sh -c`, nie `sudo`; Unit, Container, Boot und Ping-Ziel nur
  nach lokaler Auflösung gegen die echten Listen, nie ein Text des Modells als argv-Element. Ändernde Aktionen gibt es nicht.
- **`secrets::redact` hat den Modus `Log`** (`Strictness`), `scan` der Textaktionen bleibt unverändert (`Selection`). Das
  Netz vor dem Senden ist `secrets::residue` (was `redact` noch fände), nicht `scan`: `scan` meldete Zeilen wie
  `systemctl show` und sperrte jeden Bericht. Wer `secrets.rs` ändert, hält die Tests der Textaktionen und den
  Fehlalarm-Korpus (Kopf von `src/diagnose/filter.rs`) grün.
- **Schema der Antwort flach wegen der Grenze der strict-Grammatik:** ein `anyOf` über die damals 34 Proben lehnte die API ab
  („compiled grammar is too large“), und das Produkt fiel still auf „ohne strict“ zurück. Darum `catalog::flat_schema` (ein
  Objekt, `probe` als Aufzählung, jeder Parameter Pflichtfeld „Wert oder null“; Rust entfernt `null` und prüft mit
  `parse_value`). Nicht zu Vereinigungen zurückbauen.
- **Docker nur über den lokalen Socket**, mit eigenem leerem `DOCKER_CONFIG` (`$XDG_RUNTIME_DIR/dictate/diag-docker-config`,
  0500) und `DOCKER_HOST=unix:///var/run/docker.sock`; die Gruppe `docker` ist root-gleich, darum nur lesende Befehle mit
  fester Vorlage (nie `Env`, `Labels`, `exec`).
- **Ein HTTP-Stack:** alle Netzaufrufe (KI gestreamt und am Stück, `/models`, `AiTest`, Modell-Downloads) laufen über
  `src/http.rs` (eigenes HTTP/1.1, rustls *ring* + webpki-roots, `httparse`, Socket-Fristen 50 ms, abbrechbarer
  Connect mit Happy Eyeballs, DNS im Hilfsthread). Kein ureq mehr: ureq 2 kann Gesamtfrist, Leerlauf und sofortigen
  Abbruch nicht zugleich (Begründung im Modulkopf und `PLAN-actions.md`, Aufgabe 7). Nach jedem Socket-Aufruf
  `Limits` prüfen; KI folgt nie Weiterleitungen, nur Downloads über `get_following_https` (nur https, ohne
  Zugangsdaten bei Ursprungswechsel). Test-Wurzeln nur über `test_tls` unter `cfg(test)`, Produktion immer
  webpki-roots; nie „Zertifikat ignorieren“.
