# Plan: Baustein 3 „Befehlspalette mit freier Sprache“

> **Archiv (W7, 21.11.2026):** Beschreibt den Stand unter AwesomeWM/X11; AwesomeWM, die X11-Wege und die CLIs sind
> entfernt, dictate läuft unter niri (Wayland). Gültig sind `README.md` und `PLAN-wayland.md`; der Inhalt hier bleibt
> unverändert.
>
> **Unter niri (W5, 22.11.2026):** Das Panel ist neu in `dictate-ui` gebaut (Automat `ui::cmdpal`, Fenster
> `pal_view.rs`, Ausführer `ui::pal_exec` mit `pal_host.rs`), Taste `Mod+Alt+Space`; Lua (`palette.lua`,
> `palette_exec.lua`) und `dictate-palette` sind fort. Protokoll `pal_*`, Katalog, Resolver und KI-Pfad gelten weiter,
> mit Arbeitsflächen statt Tags (`view_workspace`, `move_window_to_workspace`, `previous_workspace`, `column_display`
> statt `set_layout`; `sticky`, `ontop`, `claude_usage` gestrichen). Entscheidungen, Lesarten und Stand:
> `PLAN-wayland.md` 6g.

Arbeitsplan für `ROADMAP.md` Baustein 3. Vorgabe des Users: keine Abkürzungen, immer die fachlich richtige Lösung.
Aufbau wie `PLAN-actions.md`. Geprüft wird mit `scripts/check.sh` (Subagenten nur `--only …` für ihre Schritte, die
volle Suite einmal am Ende in der Hauptsitzung).

## Stand (04.11.2026)

| # | Aufgabe | Stand |
|---|---|---|
| 1 | Katalog und Absichten (Rust-Typen, Validierung) | fertig (db474b0) |
| 2 | Lokaler Resolver ohne Modell (Regeln, Fuzzy, Häufigkeit) | fertig (3976717) |
| 3 | Quellen: Apps (.desktop), Projekte, Verlauf | fertig (abd6607) |
| 4 | KI-Pfad: freie Sprache → Absicht | fertig (fdcd268, ed6e384, 479af7e) |
| 5 | Protokoll, Daemon, CLI `dictate palette` | fertig (451920a, ohne KI) |
| 5b | Schlankes Client-Binary `dictate-palette` | fertig (6e9950e) |
| 6 | Awesome-Panel `palette.lua`, Mod+Space | fertig (497036c, 2d9ab7c, 9b206e8, 19aa854) |
| 7 | Ausführen in Lua (Fenster, Tags, Apps, Projekte, System) | fertig (f268664) |
| 8 | Stimme im Panel | fertig (26d7cc4) |
| 9 | Text und KI (Frage, Textaktionen, Zwischenablage) | fertig (7791ae7) |
| 10 | Einstellungsfenster, Doku, Xephyr-Test | fertig (5b906c2, 215e70c) |

## Antworten des Users (04.11.2026)

- **Modell:** Cloud, einstellbar. Vorgabe Claude Haiku 5.5 über die vorhandenen KI-Presets, in den Einstellungen auf
  lokal (Ollama, LM Studio) umstellbar. An das Modell gehen **nur die Eingabe und der Katalog**, keine Fenstertitel,
  keine Projekt- oder App-Listen.
- **Katalog v1:** alles, was smart und praktisch ist: Fenster und Tags, Programme und Projekte, System (harmlos;
  Abmelden/Neustart/Ausschalten nur mit Bestätigung), Text und KI.
- **Projekt öffnen** = Claude-Code-Sitzung im Projektordner (`alacritty --working-directory <dir> -e claude`).
- **Aufruf Mod+Space.** Layout weiterschalten wandert auf Mod+Shift+Space; das bisherige „vorheriges Layout“ dort
  entfällt (rückwärts über das Layout-Menü Mod+Alt+Space oder die Palette). Stimme per gehaltenem Mod+e im Panel.

## Entscheidungen

- **Erweiterung dieses Binaries** wie bei den Textaktionen: Unterbefehl `dictate palette …`, Socket-Befehle `pal_*`,
  Modul `src/palette/`. Wiederverwendet: Daemon, Config (`[palette]`, `Config::save`), `ai::ask`, Geheimnisfilter
  (`src/actions/secrets.rs`), Streaming-Socket, Panel-Bausteine und Design aus `actions.lua` (`DESIGN.md`).
  Damit ist die offene Frage aus `ROADMAP.md` (eigenes Binary oder nicht) für Baustein 3 entschieden.
- **Absichten statt Befehle.** Das Modell liefert nie ausführbaren Text, sondern eine Absicht aus dem festen Katalog,
  z. B. `{"intent":"focus_window","query":"firefox"}` oder `{"intent":"open_project","query":"dataroom"}`. Parameter
  sind Freitext-*Suchbegriffe*, die **lokal** gegen echte Daten aufgelöst werden (Fenster, Tags, Apps, Projekte). So
  bleiben Fenstertitel und Projektnamen auf dem Rechner, und das Modell kann kein Ziel erfinden: findet der Resolver
  nichts, gibt es keinen Treffer, nie eine Ausführung.
- **Validierung in Rust** ist die einzige Schranke, unabhängig vom Anbieter: Schema je Absicht (Pflichtfelder, Typen,
  Bereiche wie Tag 1–9 oder Lautstärke 0–100), unbekannte Absicht oder Felder = Fehler. Anthropic über Tool-Use mit
  festem Schema, OpenAI-kompatibel über `response_format` JSON-Schema, Ollama über `format` (Schema). Antwort ist in
  jedem Fall nur Eingabe für die Validierung.
- **Zwei Stufen:** (1) Lokaler Resolver bei jedem Tastendruck, ohne Modell, Ergebnisliste sofort (< 10 ms Ziel).
  (2) Erst wenn der User Return drückt und kein lokaler Treffer sicher ist (Schwelle), oder mit Tab „Ask AI“, geht die
  Eingabe ans Modell. Kein Modellaufruf beim Tippen.
- **Ausführen nur in Lua bzw. per argv.** Lua führt Fenster-, Tag- und Layout-Absichten selbst aus
  (`awful`-API). Programme starten per argv-Liste (`awful.spawn({ ... })`), nie über eine Shell; `.desktop`-`Exec` wird
  in Rust nach der Desktop-Entry-Spec zerlegt (Feldcodes `%f %u …` entfernt, Quoting nach Spec), ungültige Einträge
  fallen raus. Keine Modellantwort erreicht `sh -c` (Sicherheitsregel aus `ROADMAP.md`).
- **Bestätigung:** Was das Modell geliefert hat, steht als Vorschauzeile im Panel („→ Tag 3 anzeigen“), Return führt
  aus. Absichten mit Klasse `confirm` (Abmelden, Neustart, Ausschalten, Fenster schließen) brauchen eine zweite
  Bestätigung, auch bei lokalem Treffer. Nach Stimme wird nie automatisch ausgeführt.
- **Kontext für den Resolver** schickt Lua per stdin an das CLI (Tags mit Index/Name/Screen, Clients mit
  Fenster-ID, Klasse, Titel, Tag), nie per argv; das CLI reicht ihn an den Daemon. Er bleibt nur für die Anfrage im
  Speicher und wird nie geloggt oder an ein Modell geschickt.
- **Verlauf/Häufigkeit (frecency)** in `~/.local/state/dictate/palette.json` (seit W6 `frecency.json`, `PLAN-wayland.md`
  6h Block A): nur gewählte Ziel-IDs (App-ID,
  Projektpfad, Absicht) mit Zähler und Zeit, **kein Eingabetext**. Abschaltbar, Datei 0600.
- **Panel:** eigenes Modul `awesome/dictate/palette.lua`, Eingabe über `awful.prompt.run` in einem eigenen Textbox-
  Widget (Cursor, Umlaute, Löschen, Verlauf mit Pfeil hoch), `changed_callback` stößt den Resolver an (asynchron,
  veraltete Antworten werden verworfen). Fokus bleibt im Zielfenster wie beim Aktions-Panel. Maus: Klick auf Eintrag
  führt aus. Lua blockiert nie.
- **Geheimnisse:** Eingaben gehen vor jeder Cloud-Anfrage durch den Geheimnisfilter; ein Treffer sperrt die Cloud
  (wie `secrets = "local_only"` bei den Textaktionen). CLIPBOARD wird nur auf die ausdrückliche Absicht
  `explain_clipboard` gelesen, dann mit Filter und Vorschau vor dem Senden. Das ist eine zweite, bewusste Ausnahme von
  der Regel „nie CLIPBOARD lesen“ in `CLAUDE.md` (Fallen) und wird dort mit Aufgabe 9 eingetragen; der Inhalt wird nie
  geloggt, nie in `history.rs` und nie ohne Vorschau gesendet.

## Katalog v1

Klasse: `run` = sofort mit Return, `confirm` = zweite Bestätigung, `ai` = Antwort im Panel.

| Gruppe | Absicht | Parameter | Klasse | Ausführung |
|---|---|---|---|---|
| Fenster/Tags | `view_tag` | `tag` (1–9 oder Name) | run | Lua |
| | `focus_window` | `query` | run | Lua, Tag wechseln + fokussieren |
| | `move_window_to_tag` | `tag`, optional `query` (sonst fokussiertes) | run | Lua |
| | `set_layout` | `layout` (aus `awful.layout.layouts`) | run | Lua |
| | `toggle_window` | `what`: floating, fullscreen, maximized, sticky, ontop | run | Lua |
| | `close_window` | optional `query` | confirm | Lua |
| | `previous_tag` | – | run | Lua (`awful.tag.history.restore`) |
| Programme/Projekte | `launch_app` | `query` | run | argv aus `.desktop` |
| | `open_project` | `query` | run | `alacritty --working-directory <dir> -e claude` |
| | `open_folder` | `query` (Projekt) | run | Dateimanager per `xdg-open` |
| | `open_url` | `url` (nur http/https) | run | `xdg-open` |
| | `web_search` | `query` | run | Suchmaschine aus Config, Query URL-kodiert |
| System | `lock` | – | run | `loginctl lock-session` (xss-lock) |
| | `volume` | `set` 0–100 oder `step` ±, `mute` | run | wie die Lautstärke-Tasten in `rc.lua` |
| | `media` | play_pause, next, previous | run | `playerctl` |
| | `show_widget` | doorcam, claude_usage, clipboard_history, emoji | run | vorhandene Lua-Funktionen |
| | `dictate_settings` | optional Seite | run | `dictate-settings` |
| | `screenshot` | full, window, area | run | vorhandenes Werkzeug (prüfen, was `rc.lua` nutzt) |
| | `logout`, `reboot`, `poweroff`, `suspend` | – | confirm | wie `rofi-power` |
| Text/KI | `ask` | `question` | ai | Streaming ins Panel (wie freie Textaktion) |
| | `text_action` | `action` (Katalog der Textaktionen) | ai | öffnet Mod+i-Ablauf mit vorgewählter Aktion |
| | `explain_clipboard` | – | ai | CLIPBOARD lesen, Filter, Vorschau, dann Streaming |

Festes ohne Modell (Stufe 1): Zahl → Tag, `tag <name>`, App- und Projektnamen (Fuzzy), Fensterklassen/-titel,
Schlüsselwörter je Absicht auf Deutsch und Englisch („sperren“/„lock“, „lauter“, „nächster Titel“, „neustart“ …),
`?`-Präfix = `ask`, `g `-Präfix = `web_search`, URL = `open_url`.

## Aufgaben

Jede Aufgabe: Tests zuerst, `scripts/check.sh --only <Schritte>` grün, eigener Commit. Zeitbudget je Aufgabe 60–90
Minuten Agentenzeit; Halt nach zwei erfolglosen Ansätzen, dann melden. Bericht höchstens 5–12 Zeilen.

1. **Katalog und Absichten** (`src/palette/catalog.rs`, `intent.rs`). Enum der Absichten mit serde, Schema je Absicht
   (für Prompt und Tool-Use aus einer Quelle erzeugt), Klassen run/confirm/ai, Validierung mit Bereichsprüfung.
   Abnahme: Unit-Tests für jede Absicht gültig/ungültig, unbekannte Felder abgelehnt. `--only fmt,clippy,unit`.
2. **Lokaler Resolver** (`src/palette/resolve.rs`). Eingabe + Kontext + Quellen → sortierte Kandidaten (Absicht,
   aufgelöstes Ziel, Anzeigezeile, Punktzahl, sicher ja/nein). Fuzzy wie fzf (Wortanfänge, Umlaut-Faltung ä→a,
   Groß/klein egal), Schlüsselwörter DE/EN, Häufigkeit als Bonus. Abnahme: Tabellentests mit festen Kontexten
   (z. B. „ff“ → Firefox, „3“ → Tag 3, „dataroom“ → Projekt, „sperr“ → lock), Laufzeit < 10 ms bei 500 Apps/200
   Projekten (Benchmark-Test). `--only fmt,clippy,unit`.
3. **Quellen** (`src/palette/sources.rs`). `.desktop` aus `$XDG_DATA_DIRS`/`~/.local/share/applications` mit
   `NoDisplay`/`Hidden`/`OnlyShowIn`, Exec-Zerlegung nach Spec, Icons-Namen; Projekte: Git-Repos unter konfigurierbaren
   Wurzeln (Vorgabe `/home/git`, Tiefe 2, Worktrees unter `.claude/worktrees` ausgeschlossen); Cache im Daemon mit
   inotify oder mtime-Prüfung. Frecency-Datei lesen/schreiben (0600, atomar). Abnahme: Tests mit Fixture-Ordnern,
   Exec-Fälle aus der Spec. `--only fmt,clippy,unit`.
4. **KI-Pfad** (`src/palette/ai.rs`). Prompt aus Katalog erzeugt, `INPUT_GUARD` vorn wie überall, Tool-Use/JSON-Schema
   je Anbieter, Modell `palette.model` (leer = Haiku 5.5 über `effective_model`-Kette), Frist, Abbruch. Danach
   Validierung und lokale Auflösung der Suchbegriffe. Geheimnisfilter vor Cloud. Abnahme: `tests/mock_http` für alle
   drei Anbieterarten (gültig, ungültig, Absicht erfunden, Timeout); ein Live-Satz mit Haiku als eigener, budgetierter
   Messschritt (20 Sätze DE/EN, einmal, Ergebnis in `palette-ai.md`). `--only fmt,clippy,unit`.
5. **Protokoll, Daemon, CLI.** Socket-Befehle `pal_resolve` (Kontext + Eingabe → Kandidaten), `pal_ask` (→ Absicht
   oder Fehler), `pal_chosen` (Frecency), `pal_sources_refresh`. CLI `dictate palette resolve|ask|chosen` liest Kontext
   per stdin, gibt JSON-Zeilen aus. KI auf dem Verbindungs-Thread wie bei `ActRun`, Kern blockiert nie. Abnahme:
   Daemon-Tests mit `tests/daemon_common`. `--only fmt,clippy,unit`.
6. **Awesome-Panel** `awesome/dictate/palette.lua` + `setup{ palette_key = "space" }` in `init.lua`; `rc.lua`:
   Layout-Kürzel umziehen (Mod+Shift+Space = nächstes Layout), Hilfe-Popup-Gruppen anpassen (chezmoi, freigegeben).
   Eingabezeile mit `awful.prompt`, Kandidatenliste mit Icons und Tastenhinweisen, Vorschauzeile für KI-Ergebnis,
   Bestätigungszustand, Esc schließt, Sperre schließt das Panel (wie `lock.lua`). Abnahme: Xephyr-Test mit
   `awesome/test/fake-dictate`, Screenshots vom Agenten bewertet und an Daniel geschickt. `--only xephyr`.
7. **Ausführen** (`palette.lua` + `palette_exec.lua`): je Absicht eine Lua-Funktion, Ziel per Fenster-ID/Tag-Index aus
   dem Kandidaten (nie neu per Name suchen), Apps per argv-Tabelle, `confirm` mit zweitem Return. Abnahme:
   Xephyr-Tests für Tag, Fokus, Verschieben, Layout, App-Start (Fake-`.desktop`), Projekt (Fake-`claude`).
   `--only xephyr`.
8. **Stimme im Panel:** gehaltenes Mod+e bei offenem Panel leitet das `final`-Ereignis (`init.lua`, Zweig
   `kind == "final"`) in die Eingabezeile statt `type_output`; danach Resolver/KI wie getippt, keine automatische
   Ausführung. Abnahme: Xephyr-Test mit Fake-Daemon-Ereignis. `--only xephyr`.
9. **Text und KI:** `ask` und `explain_clipboard` nutzen den Streaming-Weg der Textaktionen (eigene Panel-Ansicht,
   Copy-Knopf), `text_action` öffnet das Mod+i-Panel mit vorgewählter Aktion. CLIPBOARD-Lesen nur in dieser Absicht,
   mit Filter und Vorschau. Abnahme: Unit + Xephyr. `--only unit,xclip,xephyr`.
10. **Einstellungen und Doku:** Seite „Palette“ im Einstellungsfenster (an/aus, Modell, Projektwurzeln, Suchmaschine,
    Frecency an/aus und löschen), `README`, `CLAUDE.md` (Offen), `ROADMAP.md` Stand. Abnahme: `--only gui`, dann volle
    Suite in der Hauptsitzung, Installation, Awesome neu laden, Live-Test durch Daniel.

Reihenfolge: 1 → 2 → 3 → 5 (ohne KI) → 6 → 7 ergibt eine benutzbare Palette ohne Modell; dann 4, 8, 9, 10.
Ein Schreiber zur Zeit; 1–5 berühren nur Rust, 6–8 nur Lua, daher keine Parallelarbeit am selben Ort.

## Aufwand (Schätzung)

Rust etwa 2500–3500 Zeilen mit Tests, Lua etwa 1200–1600 Zeilen mit Xephyr-Tests. Als Bauschleife mit Subagenten
(Sonnet für 1–3, 5, 7; Opus für 4 und 6 sowie Review von 4/5 wegen Netz und Geheimnissen) grob 10 Agentenläufe.

## Offen

- ~~Schwelle „sicherer lokaler Treffer“~~ festgelegt (Aufgabe 2, Modulkopf `src/palette/resolve.rs`): `confident`, wenn die
  Güte des ersten Kandidaten ≥ 88 ist (Feld exakt oder sein Anfang; nie Fuzzy oder Treffer in der Mitte), bei weniger als
  2 getippten Zeichen nur für feste Regeln („3“, `?…`, `g …`, Adresse, `volume 40`), und kein anderes Ziel nahe dran:
  ist der erste exakt (100), darf kein zweiter exakt sein, sonst ≥ 12 Punkte Abstand zur besten Güte der übrigen (der
  Neustart desselben Programms zählt nicht gegen sein laufendes Fenster, zwei Fenster einer Anwendung schon).
  Häufigkeit zählt nie dafür. Ist nichts sicher, geht Return an das Modell (Aufgabe 4). Nach den ersten Wochen im
  Alltag mit echten Eingaben nachprüfen; Konstanten `CONFIDENT_*` in `resolve.rs`.
- Aus Aufgabe 2 offen: `text_action` hat keine lokalen Schlüsselwörter (steht nicht in Stufe 1, geht über das Modell);
  `move_window_to_tag` lokal nur für das fokussierte Fenster („verschiebe firefox nach 3“ geht über das Modell);
  Adressen nur mit bekannter Endung (Liste `TLDS`, ohne `.rs`/`.md`/`.sh`/`.py`); Lua kürzt Titel auf 512 Zeichen und
  sendet höchstens 500 Fenster und 9 Tags (Rust lehnt mehr ab, `Context::from_json`).
- ~~Suchmaschine für `web_search`~~ (Vorgabe DuckDuckGo) ist `palette.web_search_url` (Aufgabe 5).
- ~~**Startzeit des CLI**~~ entschieden und umgesetzt (Aufgabe 5b, 6e9950e), Weg (a): `dictate palette …` brauchte 47 ms
  (`dictate --version`) bzw. 55 ms (`palette resolve`), weil der Prozessstart 40 ms vor `main` in den gelinkten
  CUDA-Bibliotheken (cuBLAS, cudart, ggml, Pulse) steckt. Neues Binary `dictate-palette` (`src/bin/dictate-palette.rs`)
  mit denselben Unterbefehlen `resolve|chosen|refresh|clear-history`, Ein- und Ausgaben und Exit-Codes; es bindet wie
  `dictate-settings` nur Module per `#[path]` ein (`palette/client.rs`, `sock_client.rs`, `protocol.rs`, `actions/`,
  `config.rs`) und linkt nur libc und libgcc_s (1,3 MB). `dictate palette …` bleibt und nutzt dieselbe Implementierung
  (`palette::client::run`, kein kopierter Code). **Lua ruft ab Aufgabe 6 `dictate-palette` auf**, nicht `dictate palette`.
  Gemessen (Release, Test-Daemon mit eigenem `DICTATE_SOCKET` und `--config`, 80 Aufrufe je Fall, reihum, ein Durchlauf,
  Maschine an dem Tag etwas schneller als bei der ersten Messung):

  | Aufruf | Median | p95 |
  |---|---|---|
  | `dictate-palette resolve` | 1,7 ms | 1,9 ms |
  | `dictate-palette --version` (Prozessstart) | 0,8 ms | 0,9 ms |
  | `dictate palette resolve` (vorher) | 33,2 ms | 35,4 ms |
  | `dictate --version` (vorher) | 32,2 ms | 33,9 ms |

  Ziel (< 10 ms Median) erreicht, Faktor 20; Weg (b) (Dauerverbindung aus Lua) ist damit nicht nötig. Der Test
  `slim_binary_links_no_dictation_libraries` (`tests/palette_cli.rs`) hält die Linkliste fest (`ldd`, Gegenprobe am
  `dictate`, Symbolsuche nach ggml/whisper/cublas). Wer in `palette/client.rs` etwas von `engine`, `daemon`,
  `pipeline` oder `http` braucht, macht den Start wieder langsam. `cargo tree` zeigt je Paket, nicht je Binary: das
  Paket hängt weiter von whisper-rs ab, der Link des Binaries nicht. `required-features` ist nicht nötig:
  `cargo install --path . --locked` installiert `dictate-palette` immer mit. Für Aufgabe 4: neue Unterbefehle (`ask`)
  gehören in `PalSub` (`palette/client.rs`) und gelten damit für beide Aufrufarten; die Tests in `tests/palette_cli.rs`
  laufen über `for_each_cli!` für beide.
- ~~Screenshot-Werkzeug~~ festgelegt (Aufgabe 7): flameshot, siehe „Stand Aufgabe 7“.

Hinweise aus Aufgaben 1–3 für spätere Aufgaben:
- Aufgabe 4: Schema je Anbieter anpassen (OpenAI strict ohne `oneOf`, alle Felder Pflicht mit `null`-Union;
  Anthropic strict ohne Min/Max); je Absicht ein Tool über `tool_input_schema()`. Rust-Prüfung bleibt die Schranke.
- Aufgabe 5: `dictate.service` braucht `PATH` und `XDG_CURRENT_DESKTOP` der Sitzung (sonst verwirft `TryExec` falsch).
- Aufgabe 6/7: Lua kürzt Titel auf 512 Zeichen, sendet höchstens 500 Fenster, 9 Tags, 32 Layouts.
- ~~Aufgabe 7: `rc.lua` nutzt `flameshot gui`; für `screenshot` full/window Befehl festlegen.~~ erledigt.
- Später: Desktop-Actions und Icon-Auflösung fehlen; `text_action` kennt nur eingebaute Aktionen; Schwelle für
  sichere Treffer nach einigen Wochen echter Nutzung nachprüfen.

Stand Aufgabe 4 (KI-Pfad, Rust; Panel offen), für das Lua-Panel und Aufgabe 10:
- **Aufruf:** `dictate-palette ask [--input-stdin]` (ebenso `dictate palette ask`), stdin wie bei `resolve`
  (`{"input":"…","context":{…}}`, höchstens 256 KiB, Kontext darf fehlen), Umgebung der Sitzung geht mit. Socket-Befehl
  `pal_ask` mit denselben Feldern wie `pal_resolve`. Gedacht für Return ohne sicheren lokalen Treffer
  (`confident = false`) oder Tab „Ask AI“; nie beim Tippen.
- **Antwort** (eine JSON-Zeile, Exit 0): `{"ok":true,"intent":{"intent":"focus_window","query":"firefox"},
  "candidates":[…],"web_search_url":"https://…{query}"}`. `intent` ist die geprüfte Absicht des Modells (Suchbegriffe
  wie vom Modell, getrimmt); `candidates` im Format von `pal_resolve` (`intent`, `target`, `display`, `detail`,
  `score`, `class`, `target_id`), lokal aufgelöst und nach Güte sortiert, höchstens 12, kein `confident`. Das Panel
  zeigt den ersten als Vorschauzeile, Return führt aus (Klasse `confirm` fragt nach), danach `dictate-palette chosen`
  wie bisher (die Kandidaten sind im Daemon angemeldet). Absichten ohne Ziel (System, `ask`, `web_search`,
  `open_url`, `text_action` …) liefern genau einen Kandidaten mit `target.kind = "none"`. Ausnahme `open_url`: die
  Adresse des Modells gilt nur, wenn ihr Host (klein, ohne führendes `www.`) als ganzes Wort in der Eingabe steht
  (`mentions_host` in `src/palette/resolve.rs`; „heise“ ergibt nie `www.heise.de`), sonst „No match for a web address“.
- **Fehler** (Exit 1, `{"ok":false,"error":"…"}`, nie mit Eingabe, Text des Modells oder Text des Servers): kein Ziel
  („No match for a window|an application|a project|a tag|a layout|a web address“, „No focused window“; der Suchbegriff
  wird nie ersetzt oder erfunden),
  „The AI found no matching command“ (`no_match`), „The AI chose an intent that is not in the catalog“, „The AI gave
  an unknown field for …“, „The AI gave an invalid intent: …“ (Bereich, fehlendes Feld), „AI timeout after N s“,
  „AI error: …“ (Server nur als „HTTP <Code> (<feste Meldung>)“, Schlüssel, Modell), „Nothing to ask“, „Blocked: the input contains secrets (…) and the AI server
  is in the cloud“ (zusätzlich `"secret":[Arten]`), Palette aus wie bei `resolve`, ungültiger Kontext vor jedem
  Modellaufruf. Exit 2: Daemon läuft nicht.
- **Abbruch:** das Panel beendet den `dictate-palette ask`-Prozess (Esc, neue Eingabe, Panel zu). Der Daemon merkt das
  Auflegen der Verbindung (`HangupWatch` in `src/daemon.rs`, `poll` alle 50 ms auf POLLHUP/POLLERR) und bricht den
  HTTP-Aufruf sofort ab; weitere Daten oder ein halbes Schließen des Clients (liest noch) beenden den Wächter nicht;
  ein eigener Abbruchbefehl ist nicht nötig. Wartet ein Client noch auf die Antwort, kommt `{"ok":false,
  "error":"Cancelled","cancelled":true}`. Das CLI wartet höchstens 70 s (größte `palette.timeout_ms` plus Reserve), die
  Frist setzt der Daemon.
- **Modell und Frist:** `[palette] model` (leer = `ai.model` bzw. Vorgabe des Servers, bei Anthropic Claude Haiku 5.5),
  `[palette] timeout_ms` (Vorgabe 8000, 1000 bis 60000). Server, Anbieter, Schlüssel aus `[ai]`. Laden prüft das Modell
  nicht, Speichern (`validate_new`) bei eingeschalteter Palette schon. Aufgabe 10 braucht beide Felder auf der Seite
  „Palette“.
- **An das Modell** gehen nur Eingabe und Katalog (`src/palette/ai.rs`, Prompt aus `catalog_text`, Schutzrahmen vorn):
  Claude über Tool-Use (je Absicht ein Werkzeug plus `no_match`, `tool_choice` `any` nur ohne Denken, sonst `auto`,
  strict; Tabelle `ToolCaps` in `src/pipeline/ai.rs`, Rückfall aus der Absage, siehe `SPEC-v2.md`), OpenAI-kompatibel
  über `response_format` json_schema strict, Ollama über `format`, beide mit dem flachen Schema (`intent` als
  Aufzählung, alle Parameter als Pflichtfelder mit `null`). Geheimnisfilter vor jedem Cloud-Aufruf (lokale Server
  erlaubt, wie `actions.secrets = "local_only"`). Im Log nur Modell, Name der Absicht, Zahl der Kandidaten, Dauer.
- ~~Messschritt mit Haiku (20 Sätze DE/EN)~~ erledigt, Ergebnis in `palette-ai.md` (Commit `8069365`): 19 von 20
  richtig, Median 879 ms, p95 1288 ms, keine API-Fehler. „What's the weather in Vienna“ ergibt `web_search` statt
  `ask`; das bleibt so, weil `ask` keine Live-Daten hat (Modellwissen) und die Websuche für das Wetter die richtige
  Antwort ist.
- ~~Panel-Anbindung~~ erledigt, siehe „Stand Aufgabe 4 Panel“.
- **Behoben nach dem Security-Review des KI-Pfads (drei niedrige Befunde):**
  1. Fehlertext des Servers: `AiError::Http { code, message }` (`src/pipeline/ai.rs`) hält `error.message`, aber
     `AskError::Ai` zeigt dem Client nur „AI error: HTTP <Code> (<feste Meldung>)“ (`AiError::client_text`); die
     Lernlogik für Denken und Werkzeuge liest den Text weiter, das Einstellungsfenster zeigt ihn wie bisher. Gleiches
     Muster in `src/http.rs`: Fehlermeldungen zu `Content-Encoding` und `Transfer-Encoding` zitieren den Wert aus
     dem Kopf nicht mehr. Geprüft: im strukturierten (nicht gestreamten) Pfad von `pal_ask` sonst kein Servertext
     (`stream_error` nur beim Streamen der Textaktionen; „refusing redirect to …“ nur bei Downloads).
  2. `HangupWatch` wartet ohne `events` (nur POLLHUP/POLLERR/POLLNVAL, die `poll` immer meldet): lesbare Daten und
     halbes Schließen beenden ihn nicht mehr, das spätere Auflegen bricht weiter sofort ab; keine Warteschleife ohne
     Pause (Test mit Byte, `shutdown(Write)` und beidem gegen einen Mock, der nie antwortet; ohne den Fix rot).
  3. `open_url` vom Modell: `resolve_intent` bekommt die Eingabe des Users (`Palette::resolve_intent(…, input)`),
     neuer Fehler `NoTarget::Url`. Pfad und Parameter der Adresse prüft das nicht (der Host ist der Anker); IPv6 und
     Kurzformen von IPv4 liefert das Modell nie durch, getippt erkennt sie der lokale Weg.
- **Hinweis für Aufgabe 9:** beim Ausführen von `ask` und `explain_clipboard` den Geheimnisfilter erneut gegen den
  dann gültigen Server laufen lassen (lokal oder Cloud kann sich seit `pal_ask` geändert haben; der Filter in
  `pal_ask` gilt nur für die Eingabe der Anfrage, nicht für das, was später gesendet wird).

Stand Aufgabe 4 Panel (KI im Lua-Panel), für Aufgaben 8, 9 und 10:
- **Tasten:** Return fragt die KI, wenn die Liste zur aktuellen Eingabe keinen sicheren Treffer hat (`confident = false`
  oder leer), die Eingabe nicht leer ist und der User die Auswahl nicht selbst mit ↓/↑/Tab bewegt hat (`A.picked`;
  Hover zählt nicht); sonst führt Return wie bisher aus. Läuft `resolve` noch, entscheidet Return nach der Antwort.
  **Ctrl+Enter** fragt jederzeit, auch bei sicherem Treffer: Tab war schon mit der Auswahl belegt (Aufgabe 6), darum
  nicht Tab wie in „Entscheidungen“ geplant. Fußzeile: „Enter Ask AI“ bzw. „Enter Run“ plus „Ctrl+Enter Ask AI“.
- **Zustände:** `A.asking` (Prozess läuft: „Asking AI“ mit Pixel-Wait und Sekunden, Liste ausgeblendet, Return tut
  nichts), `A.preview` (Kandidaten der KI ersetzen die Liste, Label „AI suggestion“, erste Zeile „→ …“ ausgewählt, bei
  `open_url` die volle Adresse aus `intent.url` umbrochen statt der gekürzten Anzeige; Return/Klick führt über
  `api.execute` aus, `confirm` fragt nach, `ctx.ai = true`), `A.ask_err` (rote Meldung über der lokalen Liste; Panel
  offen, Return fragt erneut). Esc bricht zuerst die Bestätigung, dann die KI-Anfrage ab, sonst schließt es. Jede
  Änderung der Eingabe (auch Verlauf ↑) bricht ab und nimmt Vorschau und Fehler weg; eine `resolve`-Antwort ändert
  eine stehende Vorschau nie.
- **Abbruch und Fristen:** Esc, neue Eingabe, Schließen, neue Anfrage: `awesome.kill(pid, SIGTERM)`, der Daemon bricht
  dann den HTTP-Aufruf ab. Antworten gelten nur, wenn sie zur laufenden Anfrage gehören (`A.ask_seq`, `A.gen`), sonst
  `pal_ask_discarded`. Lua-Frist `cfg.palette_ask_timeout` (Vorgabe 75 s, über den 70 s des CLI): Prozess beenden,
  „No answer from the AI“. Modell oder Server zeigt die Wartezeile nicht: `ask` meldet sie vorher nicht (offen).
- **Testnähte:** `fake-dictate-palette ask` (Kopf: `palette.ask` ganze Antwort, `palette.ask_code`, `palette.ask_delay`,
  `palette.ask_ignore_term`; Protokoll `palette.ask_stdin`, `palette.ask_argv`, `palette.ask_pid`). Testzustand
  `pal_asking`, `pal_ask_pid`, `pal_ask_sent`, `pal_ask_answers`, `pal_ask_discarded`, `pal_ask_cancels`,
  `pal_ask_timeouts`, `pal_preview` (Absicht), `pal_preview_input`, `pal_enter` (ask|run|wait), `pal_ask_ticker`,
  `pal_ask_error`. Xephyr-Test `awesome/test/palette_ai.sh` (nach `palette_exec.sh`, allein
  `DICTATE_TEST_ONLY=palette_ai`), Screenshots nach `/home/data/stt-test/logs/palette-ai-shots/`
  (`DICTATE_AI_SHOTS`). Für Aufgabe 8: Text der Stimme über `set_text` + `request` landet wie getippt, Return
  entscheidet danach wie oben; nie automatisch fragen oder ausführen.

Stand Aufgabe 6 (Panel ohne Ausführen), für Aufgaben 7, 8 und 10:
- **Modul** `awesome/dictate/palette.lua` (Kopf beschreibt Ablauf und Entscheidungen), eingebunden in `init.lua` über
  `setup{ palette_key = "space" }` (false = kein Kürzel) und `dictate.keys(modkey)`; `dictate.palette()` öffnet per
  Maus/Menü. Client `cfg.palette_cmd`, Vorgabe `dictate-palette` im Verzeichnis von `cfg.cmd` (echte `rc.lua`:
  `~/.cargo/bin/dictate-palette`), sonst über PATH. Neue gemeinsame Bausteine: `style.lua` (Schrift, `markup_text`,
  aus `actions.lua` herausgelöst), `json.lua` (reines Lua, Tests `awesome/test/json_test.lua`), `spawn_with_stdin` in
  `init.lua` (auch `dictate paste` nutzt ihn).
- **Kontext:** Tags nur des Screens, auf dem die Palette offen ist (Index = Position 1–9); Fenster zuletzt fokussierte
  zuerst, `id = c.window` (X-ID), `tag` nur bei genau einem Tag dieses Screens, sonst null; `focus` = Fenster, das
  beim Öffnen fokussiert war. Titel, Klasse und Namen werden auf die Grenzen gekürzt und als UTF-8 bereinigt.
- **Schnittstelle für Aufgabe 7:** `palette.execute(candidate, ctx)` (`require("dictate")._palette.execute`, als
  Feld ersetzbar). Das Panel ist beim Aufruf schon zu, Fehler im Hook werden als Overlay-Hinweis gezeigt. `candidate`
  wie vom CLI (`intent`, `target`, `display`, `detail`, `score`, `class`, `target_id`); `ctx` = `client` (Ziel beim
  Öffnen, gültig oder nil), `screen`, `input`, `context` (der zuletzt gesendete Kontext), `web_search_url`,
  `confident`, `via` (`key`|`mouse`). Bisher meldet der Hook nur `dictate-palette chosen` und zeigt eine
  Platzhalter-Benachrichtigung (naughty). Bestätigung (`confirm`) erledigt schon das Panel.
- **Verlauf** (↑ in der ersten Zeile) nur im Speicher der Sitzung, keine Verlaufsdatei des Prompts.
- **Tastenverlust im vollen Lauf (behoben):** 8t beendet ein tippendes xdotool zwischen Drücken und Loslassen, die
  Taste bleibt im X-Server gedrückt und ihr nächstes Drücken verwirft der Server (mit XRecord belegt); `palette.sh`
  gibt darum zu Beginn hängende Tasten frei. Wackelig bleibt 8t selbst (Mod+Ctrl+e während getipptem Replace).
- **Aufgabe 8:** Text von außen in die Eingabezeile mit `set_text(text)` plus `request(text)` (wie `history_step`):
  setzt den Text im laufenden Prompt, Cursor am Ende, kein Neustart.

Stand Aufgabe 5 (ohne KI; `pal_ask` fehlt noch), für Aufgaben 4, 6, 7 und 10:
- **CLI** `dictate palette …` liest nie argv für Eingabe oder Kontext und lädt keine Config. Ausgabe: genau eine JSON-Zeile
  auf stdout (die Antwort des Daemons unverändert); Fehler, auch die des CLI selbst, als `{"ok":false,"error":"…"}` mit
  Exit 1, Exit 2 = Daemon läuft nicht (ebenfalls mit JSON-Zeile). Fehlermeldungen enthalten nie Eingabe oder Kontext.
  - `resolve [--input-stdin]` ← stdin `{"input":"…","context":{"tags":[{"index":1,"name":"web","screen":1,"selected":true}],
    "clients":[{"id":123,"class":"…","instance":"…","title":"…","tag":1,"screen":1,"focused":false}],"layouts":["tile"],
    "focus":123}}` (höchstens 256 KiB, `context` darf fehlen oder `null` sein, unbekannte Felder sind Fehler) →
    `{"ok":true,"candidates":[{"intent":{"intent":"focus_window","query":"Firefox"},"target":{"kind":"window","id":123},
    "display":"…","detail":"…","score":97.0,"class":"run","target_id":"firefox"},…],"confident":true,
    "web_search_url":"https://duckduckgo.com/?q={query}"}`. Lua setzt in `web_search_url` den prozentkodierten Begriff
    für `{query}` ein (`palette.web_search_url`: https, genau ein `{query}`, nie im Host).
  - `chosen --intent <candidate.intent.intent> --target-id <candidate.target_id>` → `{"ok":true,"recorded":true}`
    (`false`, wenn `palette.frecency` aus ist). Abgelehnt (Exit 1) wird, was der Daemon nicht selbst in einer der letzten
    32 Antworten auf `pal_resolve` angeboten hat (`palette::service::Offered`); Aufgabe 4 muss die Kandidaten von
    `pal_ask` ebenso anmelden, sonst scheitert `pal_chosen` für sie.
  - `refresh` (Programme und Projekte beim nächsten Zugriff neu lesen), `clear-history` → `{"ok":true}`.
  - Bei `[palette] enabled = false` antworten alle mit „The command palette is off (Settings › Palette)“.
- **Sitzungsumgebung:** das CLI schickt `PATH` und `XDG_CURRENT_DESKTOP` seines Prozesses (aus Awesome gestartet) in jedem
  `pal_resolve` mit (`path`, `desktop`); sie gelten für `TryExec` und `OnlyShowIn`, ändern sie sich, liest der Dienst die
  Programme neu. `SourcesConfig` brauchte dafür keine Änderung (`DesktopEnv` hat beide Felder). Seit 5b gehen auch
  `XDG_DATA_DIRS` und `XDG_DATA_HOME` der Sitzung mit (`data_dirs`, `data_home`; Typ `protocol::SessionEnv`, ersetzt
  `client::PalEnv`/`palette::SessionEnv`): ist `XDG_DATA_DIRS` in der Sitzung gesetzt, ersetzt es die Verzeichnisse des
  Dienstes (leer oder ohne absoluten Eintrag: Vorgabe der Spec), `XDG_DATA_HOME` ersetzt das des Dienstes nur als
  absoluter Wert; fehlt eine Variable, bleibt die Umgebung des Dienstes. Jede Änderung liest die Programme neu.
- **Quellen und Häufigkeit** entstehen im Daemon erst beim ersten Aufruf: der erste `pal_resolve` liest Programme und
  Projekte (gemessen 86 ms für den ganzen CLI-Aufruf, etwa 30 ms mehr als später, mit 170 Einträgen und 78 Ordnern unter
  `/home/git`); danach prüft ein mtime-Lauf höchstens alle 2 s. `palette.json` (seit W6 `frecency.json`) liegt neben `history.jsonl`. Die
  Konfiguration wirkt sofort (auch während des Diktats). Ein Vorwärmen beim Start gibt es nicht.
- **Laufzeit gemessen** (Release, Test-Daemon mit eigenem Socket und `--config`, 80 Aufrufe): Daemon und Socket 0,5 ms
  (Median), aber `dictate palette resolve` gesamt 55 ms (Median, p95 95 ms), davon 47 ms bloßer Prozessstart
  (`dictate --version`): die 40 ms stecken vor `main` in den gelinkten CUDA-Bibliotheken (cuBLAS, cudart, ggml; der
  dynamische Lader braucht nur etwa 1 ms). Das Ziel „unter 30 ms“ ist mit diesem Binary nicht zu erreichen; Entscheidung
  offen (siehe „Startzeit des CLI“ oben).

Stand Aufgabe 8 (Stimme im Panel), für Aufgaben 9 und 10:
- **Fokus/Tasten:** Der Keygrabber des Prompts schluckt Mod+e: solange er läuft, feuern die globalen Bindungen aus
  `init.lua` nicht. `on_key` (Zweig `mod[api.modkey]`) reicht darum Mod+e (halten), Mod+Shift+e (freihändig) und
  Mod+Ctrl+e (abbrechen) an `sh.voice` weiter (`press`/`toggle`/`cancel` sind dieselben Funktionen mit derselben
  Wiederholungssperre `once()` wie die Bindungen); das Loslassen von `e`, `E`, `Super_L` und `Super_R` geht über
  `keyreleased_callback` an `sh.voice.release` (Super zuerst loslassen beendet ebenso, getestet). Mod+Ctrl+Shift+e
  (Einstellungen) bleibt im Panel unbelegt. `start --app <Klasse>` und damit Profil und KI-Modus des Diktats bleiben
  unverändert (Klasse des Zielfensters, das den Fokus behält).
- **Ziel entscheidet der Start, nicht das final:** `S.voice_next` (beim Auslösen von Halten/Toggle: Panel offen =
  `"palette"`) wird beim Zustandswechsel nach `loading`/`listening` zu `S.voice` (`"palette"` | `"discard"` | nil =
  Fenster). So bleibt ein Diktat ins Fenster eines (Regression getestet), und eine Aufnahme des Panels wird nie ins
  Fenster getippt. Ein fehlgeschlagener Start (`dictate` Exit ≠ 0) verwirft die Vormerkung.
- **final bei offenem Panel:** `palette.voice_text(text)`: Text bereinigen, `set_text` (Cursor am Ende) und damit
  `on_changed` → `request` (lokaler Resolver wie beim Tippen), danach entscheidet Return wie gewohnt (sicherer Treffer:
  ausführen, sonst KI-Vorschau). Nie `type_output`, nie `dictate paste`/`synth`, nie ausführen, nie die KI fragen. Der
  Text **ersetzt** die Eingabe ganz (kein Anhängen): eine Zeile ist ein Befehl, und ein zweiter Versuch soll den ersten
  überschreiben. Pipeline und KI-Modi des Diktats gelten unverändert (es kommt der fertige Text des Daemons).
  Bereinigung (`one_space`): Steuerzeichen, Zeilen- und Absatztrenner (auch U+0085, U+2028, U+2029) zu einem Leerzeichen,
  Leerraum zusammengezogen, getrimmt, auf 1000 Zeichen (`MAX_INPUT_CHARS`, wie die Eingabe) gekürzt; ein Sprachbefehl
  „neue Zeile“ ergibt also ein Leerzeichen. Die Tasten des final (BackSpace für „undo that“) gehören zum Fenster und
  werden im Panel verworfen; leerer Text ändert nichts.
- **Anzeige:** Das Overlay bleibt aus, solange die Aufnahme dem Panel gehört (`S.voice ~= nil`, `render()` in
  `init.lua`); Fehlermeldungen (`S.notice`) erscheinen weiter dort. Begründung: das Panel hat 12 Zeilen und füllt dann die
  ganze Bildschirmhöhe (Screenshot `t4-palette-many`), ein Overlay am Rand läge garantiert darüber, und der Blick ist
  ohnehin an der Eingabezeile. Statt der Liste zeigt das Panel einen Block wie bei „Asking AI“: Pixelwelle mit Pegel,
  Titel „Getting ready“/„Listening“/„Transcribing“, eine Zeile Mitschrift (vorn gekürzt, stabiler Teil weiß, Rest grau;
  ohne Mitschrift „Speak your command. Esc cancels.“), Fuß „Esc Cancel“. Alles wird an Ort und Stelle aktualisiert
  (`voice_apply`), das Panel behält seine Größe. Währenddessen tun alle Tasten außer Esc und den drei Kürzeln nichts
  (Return führt nie die verdeckte Liste aus, Tippen würde vom Text ersetzt); eine wartende Bestätigung und eine laufende
  KI-Anfrage werden beim Start der Aufnahme beendet.
- **Esc und Schließen (Entscheidung: verwerfen statt tippen, das Ziel ist weg):** Esc bricht zuerst die Aufnahme ab
  (`sh.voice.abort` → `cancel_recording`, die Eingabe bleibt wie sie war; bei noch gehaltenem Mod+e reicht Mod+Esc), ein
  weiteres Esc schließt. Jedes andere Schließen (Mod+Space, Klick daneben, Fokuswechsel, Sperre, Ausführen) ruft
  `voice_panel_closed` und bricht eine Aufnahme des Panels ab. Mod+Ctrl+e bricht ebenfalls ab. Nach jedem Abbruch steht
  `S.voice = "discard"`, bis das final der Sitzung da ist: ein final mit Text, das beim Abbruch schon unterwegs war,
  wird verworfen und nie ins Fenster getippt (Test mit `nocancel` im Fake). Der Abbruch nutzt `cancel_recording`
  (Autorepeat-Sperre `cancel_seen`, kein Neustart bei noch gehaltener Taste), nicht `stop_typing`: ein laufendes Tippen
  eines früheren Diktats bleibt unberührt.
- **Testnähte:** `fake-dictate`: Dateien `final.text` (roher Text des final, mit Umbrüchen), `final.keys`, `fin_ms`,
  `nocancel` (cancel wird ignoriert), Befehl `final TEXT [KEYS]`, `spawn.log` (eine Zeile je `paste`/`synth`). Zustand:
  `pal_voice` (loading|listening|finalizing|-), `pal_voice_shown`, `pal_voice_inputs`, `pal_voice_aborts`,
  `pal_voice_last`; `vdest`, `vnext`, `vinputs`, `vdrops` (init.lua). Xephyr-Test `awesome/test/palette_voice.sh` (nach
  `palette_ai.sh`, allein `DICTATE_TEST_ONLY=palette_voice`), Screenshots nach
  `/home/data/stt-test/logs/palette-voice-shots/` (`DICTATE_VOICE_SHOTS`).
- **Offen:** (1) Wird ein Diktat ins Fenster gestartet und öffnet der User das Panel vor dem final, trifft das final auf
  den Keygrabber (xdotool/Ctrl+V gehen an den Prompt statt ans Fenster); unspezifiziert, seltener Wettlauf von etwa einer
  Sekunde. ~~(2) Der Daemon schreibt jedes final in den Diktatverlauf (`history`), auch das Diktat ins Panel~~ erledigt mit
  Aufgabe 9 (`--target palette`, siehe „Stand Aufgabe 9“). (3) Das Panel nennt das Kürzel nirgends (Fußzeile oder
  Platzhalter); README mit Aufgabe 10. (4) „undo that“ im Panel ohne Wirkung. (5) Live-Test am echten Desktop (Brio,
  picom, echte Stimme) fehlt.

Stand Aufgabe 7 (Ausführen), für Aufgaben 8, 9 und 10:
- **Module** `awesome/dictate/palette_exec.lua` (Kopf: Regeln und Befehle je Systemabsicht) und `palette_url.lua` (reines Lua:
  Prozentkodierung nach RFC 3986, Suchadresse, http/https-Prüfung; Test `awesome/test/palette_url_test.lua`, läuft in
  `run.sh`). `palette.lua` hängt es über `api.exec` an `api.execute`; `exec.execute(candidate, ctx, on_success)` ruft
  `on_success` (= `dictate-palette chosen`) **nur nach Erfolg**. Jede Absicht ist eine Funktion mit `done(ok, text)`, auch
  asynchrone (pactl, systemctl, loginctl melden beim Programmende); jede läuft unter pcall, Rückrufe ebenso. Fehler,
  fehlendes Ziel oder Programm: kurze englische naughty-Meldung, kein `chosen`. `dictate._debug()` hat `pal_x_ok`,
  `pal_x_failed`, `pal_x_intent`, `pal_x_msg`. Testnaht `dictate._palette.exec.hooks.quit` (Abmelden; sonst
  `awesome.quit()`).
- **Rust (Target::App)**: `Target::App { id, argv, working_dir, terminal }`: `argv` ist die vom Daemon nach der Desktop Entry
  Specification zerlegte `Exec`-Zeile, `working_dir` ist `Path` (fehlt im JSON, wenn leer), `terminal` ist `Terminal=true`.
  Nur der Kandidat trägt es; die Häufigkeit kennt weiter nur `target_id`. Lua bekommt nie einen Shell-String.
- **Ziele** immer aus dem Kandidaten: Fenster per `client.window == id`, Tag per Index auf `ctx.screen` (Position 1 bis 9; ein
  Name vom Modell wird nur dort exakt gesucht), Programm per argv, Projekt per Pfad. Fensterabsichten ohne Zielfenster wirken
  auf `ctx.client`; ist das Zielfenster weg, gibt es „That window is gone“, nie eine Ersatzwahl. `focus_window` zeigt den Tag
  (`first_tag:view_only()`), wechselt den Screen und sendet `request::activate` mit Kontext `palette`.
- **Programme starten** nur als argv-Tabelle (`awful.spawn(argv, false)`), vorher prüft Lua, ob das Programm im PATH der Sitzung
  liegt. Awesome 4.3 hat keinen Arbeitsordner für `awful.spawn`: `env -C dir argv…`; `Terminal=true` → `alacritty
  [--working-directory dir] -e argv…`. `open_project`: `alacritty --working-directory <Pfad> -e claude`; `open_folder` und
  `open_url`: `xdg-open` (URL nur http/https, noch einmal in Lua geprüft); `web_search`: `ctx.web_search_url` mit
  prozentkodiertem Begriff (https, genau ein `{query}`, nie im Host, sonst Fehler).
- **System** (wie `rc.lua` und `~/.local/bin/rofi-power`): `lock` = `pgrep -x xss-lock`, dann `loginctl lock-session`, sonst
  `lock-screen` (rofi-power macht es ebenso; ohne xss-lock täte `lock-session` nichts); `volume` = `pactl set-sink-volume
  @DEFAULT_SINK@ N%` bzw. `set-sink-mute … 1|0` (die Tasten gehen über continuity.audio an dasselbe pactl); ein Schritt
  liest erst `pactl get-sink-volume` und wird nie über 100 % angehoben (relativ `+N%`/`-N%`, die Balance bleibt; bei 100 %
  Hinweis, kein Fehler); `media` = `playerctl play-pause|next|previous` (continuity.media spricht MPRIS direkt, playerctl
  wählt den Player selbst); `show_widget`: `doorcam` → `require("doorcam").toggle()`, `claude_usage` →
  `require("claude_usage").toggle_popup()` (per pcall(require), fehlt das Modul: „… is not available“), `clipboard_history`
  → `env CM_LAUNCHER=rofi clipmenu -theme-str …` (Thema wie Mod+Shift+v), `emoji` → `rofi -modi emoji -show emoji`;
  `dictate_settings` → `sh.settings(page)` (`dictate-settings --page <Seite>`); `logout` = `awesome.quit()` (erst
  `chosen` melden, dann beenden); `reboot`/`poweroff`/`suspend` = `systemctl …`. **suspend** ist auf blub per `sleep.conf`
  gesperrt (rofi-power hat ihn deshalb nicht): systemd lehnt ab, die Palette zeigt „Suspend failed: …“ und zählt es nicht.
- **screenshot** (flameshot 14): `area` = `flameshot gui`, `full` = `flameshot full --clipboard` (alle Bildschirme in die
  Zwischenablage; ohne `--clipboard` oder `--path` wüsste flameshot nicht, wohin), jeweils mit `--delay 300`, damit das
  ausblendende Panel nicht im Bild landet. **flameshot hat keinen Fenstermodus** (nur gui, screen, full, launcher, config).
  `window` = `flameshot gui --region WxH+X+Y --delay 300` mit dem Bereich des Zielfensters (Rahmen eingeschlossen, auf die
  Bildschirmfläche begrenzt) schon ausgewählt; der Editor öffnet sich, Return bestätigt, der Bereich bleibt anpassbar.
  Fenster = Zielfenster des Kandidaten, sonst das beim Öffnen fokussierte; ohne beides Meldung. `--region` ist mit der
  echten flameshot-Version 14.0.0 nur gegen `--help` belegt, im Test steht ein Fake; beim ersten echten Aufruf prüfen,
  ob das Bild exakt das Fenster zeigt (HiDPI, mehrere Bildschirme).
- ~~**KI-Absichten** (`ask`, `text_action`, `explain_clipboard`): Meldung „Coming soon: …“~~ erledigt, siehe „Stand Aufgabe 9“.
- **Xephyr-Test** `awesome/test/palette_exec.sh` (von `run.sh` nach `palette.sh` eingelesen, allein mit
  `DICTATE_TEST_ONLY=palette_exec`): Kandidaten gehen über einen Lua-Kanal (`$DICTATE_TEST_LUA`, Test-`rc.lua`) an
  `dictate._palette.execute`, vier Fälle über das echte Panel (`palette.cands` im Fake-Daemon). Fake-Programme in
  `awesome/test/bin/` (Symlinks auf `_fake`, schreibt argv und Arbeitsordner nach `exec.log`) stehen nur in der PATH der
  Awesome-Sitzung des Tests vorn, nie im Test selbst; `doorcam` und `claude_usage` sind in der Test-`rc.lua` Fakes
  (`package.loaded`/`preload`).

Stand Aufgabe 9 (Text und KI), für Aufgabe 10:
- **Ein Panel für alles Gestreamte:** `ask`, `explain_clipboard` und `text_action` öffnen das Panel der Textaktionen
  (`actions.lua`, `api.open_with(opts)`, in `init.lua` als `sh.text_panel`) statt einer Kopie in `palette.lua`: Streaming,
  Markdown, Scrollen, Copy (auch je Codeblock), Abbruch, Sperre, Fokus im Zielfenster und Position über dem Zielfenster
  gibt es dort schon. Neu ist nur die Quelle (`opts.source`: `selection` | `question` | `clipboard`), danach Kopf
  („Ask“, „Explain clipboard“), Texte der Meldungen und das Verhalten von Esc/Zurück. `palette_exec.lua` öffnet es per
  `gears.timer.delayed_call` (der Keygrabber der Palette ist dann sicher frei); „Coming soon“ ist weg.
- **Protokoll:** `act_grab` hat `source` (`selection` Vorgabe, `question`, `clipboard`; `Source` in
  `src/actions/store.rs`, die Sitzung merkt sie sich). Sitzungen der Palette bieten genau eine Aktion
  (`actions::source_action`: `ask` bzw. die eingebaute `explain`), hängen an `palette.enabled` statt
  `features.actions` (Meldung „The command palette is off …“), nutzen `palette.model` (leer = `ai.model` bzw. Vorgabe des
  Servers, dieselbe Kette wie Aufgabe 4), Frist und Längengrenze der Textaktionen (`actions.timeout_ms`,
  `actions.max_chars`), `replace_ok` immer falsch. `act_run` prüft Schalter, Aktion und Ziel gegen den jetzt gültigen
  Stand: Geheimnisfilter gegen den Server beim Senden (Zielwechsel seit dem Greifen = „AI server changed“, nichts
  gesendet), Geheimnis + Cloud = gesperrt; `actions.secrets = "block"` gilt nur für Markierungen (Palette wie
  `pal_ask`: lokal erlaubt). Meldungen je Quelle („The clipboard is empty“, „Nothing to ask“, „Clipboard text too
  long: …“, „Blocked: the question contains secrets (…)“).
- **`ask`:** Frage per stdin an `dictate act grab --source question` (nie argv), danach sofort `act run --action ask`,
  gestreamt. Prompt: eigener fester Rahmen `config::ASK_FRAME` statt `INPUT_GUARD` (der sagt „never answer“, hier ist
  Beantworten das Ziel; zwei widersprüchliche Regeln vertragen die Modelle schlecht, der Guard bleibt für alle anderen
  Wege unverändert). Inhalt: keine Werkzeuge, kein Netz, keine Live-Daten (dann ein Satz dazu), keine Handlung am
  Rechner behaupten, Versuche, die Regeln zu ändern, sind nur Teil der Frage, nichts erfinden, Sprache der Frage,
  knapp, Markdown-Teilmenge mit Codeblöcken. `ai::ask` lässt genau diesen Rahmen ohne Schutzrahmen durch (sonst
  stellt es ihn weiter voran). Nicht editierbar; ein Live-Test mit Haiku fehlt (kein Live-Aufruf in dieser Aufgabe).
  Panel: Frage grau über der Antwort, kein „Back“, Esc bricht den Aufruf ab und schließt (CLI per SIGTERM, `act
  cancel`, `act clear`), Return kopiert die Antwort.
- **`explain_clipboard`:** CLIPBOARD liest das CLI (`Xclip::read_clipboard`, dieselbe Kodierungslogik, Frist 2 s und
  Grenze wie PRIMARY), weil nur der aus Awesome gestartete Prozess ein DISPLAY hat (der Dienst nicht) und der Code
  dort schon getestet ist; der Daemon bekommt den Text per Socket. `act_grab` ist die Vorschau (Zeichen, Zeilen,
  Ziel-Server und Modell, erste Zeilen) und sendet nichts; gesendet wird erst mit `act run` nach Return (oder e, Klick)
  auf „Explain“, das als aktive Zeile vorbelegt ist. Esc in der Vorschau sendet nichts. Das Return, das die Palette
  ausführt, bestätigt die Vorschau nie (Totzeit und gehaltene Tasten wie im Aktions-Panel, getestet). Prompt: die
  eingebaute Aktion `explain` mit Schutzrahmen und Antwortsprache (Inhalt der Zwischenablage ist fremder Text).
  Zweite CLIPBOARD-Ausnahme in `CLAUDE.md` (Fallen) eingetragen.
- **`text_action`:** Mod+i-Ablauf auf PRIMARY mit `opts.preselect`; nach dem Greifen startet die Aktion ohne Auswahl,
  wenn sie erlaubt ist (gesperrt: Sperrblock; unbekannt: „Unknown text action: …“; Textaktionen aus: „Text actions are
  off (Settings › Actions)“ mit Anleitung im Panel).
- **`chosen`** für alle drei erst, wenn eine Antwort da ist (`opts.on_success`, einmal, ohne Text); Fehler, Sperre und
  Abbruch zeigt das Panel, ohne zweite naughty-Meldung und ohne `chosen`.
- **Diktat der Palette nicht im Verlauf:** `start`/`toggle` mit `"target":"palette"` (`Request::PaletteVoice`, CLI
  `dictate start|toggle --target palette`); Lua setzt es nur, wenn die Aufnahme dem Panel gehört (`S.voice_next ==
  "palette"` in `run_start`/`do_toggle`). Der Kern merkt sich das Ziel je Sitzung (`rec_window`, auch für den
  aufgeschobenen Start) und ruft für die Palette `record_final` nicht auf: kein Verlauf und nicht als getippt gezählt
  (ein „rückgängig“ im Fenster löscht danach nichts, was nie dort ankam; vorher hätte es Zeichen gelöscht).
- **Tests:** `tests/daemon_actions.rs` (Frage gestreamt mit `ASK_FRAME` und Palette-Modell, Abbruch < 500 ms,
  Geheimnis sperrt nur die Cloud, Zielwechsel nach dem Greifen, Zwischenablage erst nach `act_run`, Grenze, Schalter),
  `tests/daemon_pipeline.rs` (Palette-Diktat nicht im Verlauf, „rückgängig“ ohne Wirkung), `tests/act_cli.rs`
  (Ersatz-xclip: nur `-selection clipboard`, Frage von stdin ohne xclip; echtes xclip unter Xvfb liest CLIPBOARD nur mit
  `--source clipboard`), Unit-Tests in `protocol.rs`, `store.rs`, `actions/mod.rs`. Xephyr `awesome/test/palette_textai.sh`
  (nach `palette_voice.sh`, allein `DICTATE_TEST_ONLY=palette_textai`; Fake: `act grab --source …`, `clip.src`,
  `act_question.log`, `act_source.log`, `act_grab.argv`, `target.log`), Screenshots nach
  `/home/data/stt-test/logs/palette-textai-shots/` (`DICTATE_TEXTAI_SHOTS`). Testzustand: `act_source`, `act_action`,
  `act_back`, `act_hint`, `act_opened_question`, `act_opened_clipboard`, `geo_hint`.
- **Offen:** (1) `ASK_FRAME` ohne Live-Messung (Antwortqualität, Länge, Verhalten bei „ignore your rules“); ein
  budgetierter Messschritt wie `palette-ai.md` wäre der nächste Schritt. (2) Seite „Palette“ (Aufgabe 10): Modell gilt
  jetzt auch für `ask`/`explain_clipboard`, Frist und Längengrenze kommen von der Seite „Actions“; ob die Palette eigene
  Werte braucht, entscheidet der User. (3) Der Kopf zeigt für Palette-Sitzungen keinen Einstellungsknopf, bis es die
  Seite gibt. (4) Live-Test am echten Desktop.

Stand Aufgabe 10 (Einstellungsfenster, Panel-Feinheiten, Doku), für den Live-Test:
- **Seite „Palette“** (`src/gui/pages/palette.rs`, `dictate-settings --page palette`): Schalter (aus = nur ein Hinweis,
  wie bei History), Gruppen AI (Server schreibgeschützt mit Cloud/Local, Modell mit Auswahl und Hinweis auf das
  wirksame Modell, Frist `palette.timeout_ms`, Links zu AI und Actions), Projects (Wurzeln, Tiefe als Chips 1–4,
  „Rescan apps and projects“), Web search (Adresse, „Reset to default“), Frequent picks (`frecency`, „Clear history“ mit
  „Really delete“), Privacy (vier Zusagen), Keys (schreibgeschützt). Entscheidung umgesetzt: `ask` und
  `explain_clipboard` haben keine eigenen Felder, die Seite sagt, dass Frist und Längengrenze der Actions-Seite
  gelten und das Modell der Palette (`palette.model`, leer = `ai.model`). `SettingsPage::Palette` im Katalog
  (Absicht `dictate_settings`), der Test `settings_pages_match_the_settings_window` hält die Namen gleich;
  `palette_exec.lua` ruft `dictate-settings --page palette` unverändert auf.
- **Validierung:** Wurzeln und Suchadresse prüft das Fenster schon beim Tippen mit denselben Funktionen wie
  `Config::validate` (`PaletteCfg::check_root` neu herausgelöst, `check_search_url`); Text und Feldpfad wie vom
  Daemon (Test vergleicht beide). Eine frisch angelegte leere Wurzel wird erst beim Speichern beanstandet, mehrere
  fehlerhafte Wurzeln sind alle rot (`string_list_flagged`), der Text nennt die erste. Modellprüfung nur beim Speichern
  (`validate_new`), im Fenster ein Hinweis wie auf der Actions-Seite.
- **Fehler gefunden und behoben:** `gui::dto::Palette` kannte `model` und `timeout_ms` nicht; jedes Speichern aus dem
  Fenster hätte beide auf die Vorgabe zurückgesetzt (Test `palette_page_fields_survive_a_round_trip_through_the_daemon_config`).
- **Knöpfe zum Daemon:** `pal_frecency_clear` und `pal_sources_refresh` (Tags `PalClear`, `PalRefresh`), Ergebnis neben
  dem Knopf; aus, solange der Daemon fehlt oder er die Palette noch aus hat (`saved.palette.enabled`).
- **Mock** `tests/gui/mock_daemon.py`: `[palette]` mit allen Feldern, Validierung wie `Config::validate`/`validate_new`,
  `pal_*`-Befehle (Protokoll in `mock.log`), `MOCK_PALETTE_BAD=1` (unzulässige Werte schon beim Anzeigen),
  `MOCK_PALETTE_OFF=1`; `smoke.sh` und `shots.sh` kennen die Seite. Screenshots (Normalzustand, Fehler, Apply-Fehler,
  Clear, Rescan) in `/home/data/stt-test/logs/palette-gui-shots/`.
- **Panel:** (a) die Marke CONFIRM/AI hat in der ausgewählten Zeile Rahmen Teal 40 % (`style_row`; gray-800 auf
  black-extra war unsichtbar); (b) „↑↓ Select“ nur bei mehr als einem Kandidaten; (c) die Fußzeile nennt `Mod+e`
  „Hold to speak“ zwischen Ctrl+Enter und Esc; reicht die Zeile nicht, fällt nur dieser Hinweis weg (`footer_width`,
  `panel_width`). Gemessen im Test (Ersatzschrift, Test-Dpi): längste Zeile 929 von 962 px, bei 520 statt 640 px Breite
  fällt der Hinweis weg. Testzustand `pal_foot`, `pal_foot_w`, `pal_foot_avail`, `pal_chips`; Xephyr-Abschnitt 10l in
  `palette.sh`, Screenshots `palette-chip-selected`, `palette-one-candidate`, `palette-narrow`, `palette-footer-voice`
  (Ausschnitte in `/home/data/stt-test/logs/palette-panel-shots/`).
- **Über die Liste hinaus (klein):** Das Panel der Zwischenablage-Vorschau (Quelle `clipboard`/`question`) hat jetzt den
  Einstellungsknopf, er öffnet die Seite Palette (offener Punkt (3) aus Aufgabe 9; `actions.lua`, `render_select`),
  Test in `palette_textai.sh` (14c).
- **Doku:** `README.md` (Abschnitt Befehlspalette, Tabelle `[palette]`, Kürzel), `CLAUDE.md` (Stand, Kürzel, Offen 7),
  `ROADMAP.md` (Baustein 3).
- **Geprüft:** `scripts/check.sh --only fmt,clippy,gui,xephyr` grün (fmt 1 s, clippy 2 s, gui 34 s, xephyr 581 s); in einem
  früheren Lauf direkt nach dem Systemstart ging im Test 10b das ß von „Grüße“ verloren (bekannte Last-Anfälligkeit von
  `xdotool type`, siehe Kommentar dort), der Wiederholungslauf war grün. Gezielt dazu `cargo test --release --bin
  dictate-settings` (135 + neue Tests) und `cargo test --release --lib palette` (195). Die volle Suite (unit, xclip,
  speech, gpu) läuft in der Hauptsitzung.
- **Offen für den Live-Test (User):**
  1. Installieren: `cargo install --path . --locked`, `dictate.service` neu starten (der laufende Daemon kennt `pal_*`
     und die Seite erst danach), Awesome neu laden; `awesome.startup_errors` muss nil sein, `require("dictate")._debug()`
     `daemon_up=true`.
  2. Panel unter echtem picom (glx): Rahmen der Marke in der Auswahl, Fußzeile mit Stimmenhinweis in „Google Sans Flex“
     (der Test misst mit Ersatzschrift; bei Überlauf fällt der Hinweis von selbst weg), Position über dem Zielfenster.
  3. Stimme im Panel mit der Brio: Mod+e halten, Text in der Zeile, Enter führt aus; „undo that“ hat dort keine Wirkung.
  4. KI-Pfad live mit Haiku: Eindruck der Schwelle `CONFIDENT_*`, Antwortqualität von `ASK_FRAME` („ignore your rules“,
     Länge; ein budgetierter Messschritt wie `palette-ai.md` fehlt), Wartezeile zeigt Modell und Server nicht.
  5. `explain_clipboard` mit echter Zwischenablage und Vorschau, Sperre über xss-lock schließt das Panel.
  6. System: `flameshot --region` (Fenster-Screenshot) gegen die echte Version 14.0.0 (HiDPI, mehrere Bildschirme),
     `lock` über xss-lock, `suspend` wird von systemd abgelehnt (Meldung), `rofi-power` › Lock geht an xss-lock vorbei.
  7. Projekte: `/home/git` mit Tiefe 2 liefert die erwarteten Repositories, `alacritty --working-directory … -e claude`.
  8. Einstellungsfenster gegen den echten Daemon: Seite Palette ändern und Apply (Modell, Frist, Wurzeln), „Clear
     history“, „Rescan“; die Seite „Actions“ zeigt Frist und Längengrenze, die `ask` übernimmt.
  9. Kleinigkeiten ohne Entscheidung: ein Diktat ins Fenster, dessen final nach dem Öffnen des Panels kommt, trifft den
     Keygrabber (Wettlauf von etwa einer Sekunde); `text_action` kennt nur eingebaute Aktionen; Desktop-Actions und
     Icon-Auflösung fehlen; Adressen nur mit bekannter Endung; `move_window_to_tag` lokal nur für das fokussierte Fenster;
     der nicht gestreamte KI-Pfad der Textaktionen lässt bei Abbruch einen Thread bis zur Frist laufen.

Nach dem Live-Test (05.11.), drei Befunde von Daniel:
- **`screenshot window`: Vorauswahl verschoben, Größe richtig.** Ursache (flameshot-git r2348.344aa78e, Quelltext
  gelesen, vom Programm nur `flameshot screen --help`): unter X11 zeigt `CaptureWidget` den Editor immer auf genau einem
  Monitor (Fenster = Geometrie des gewählten Screens), `--region` ist eine Auswahl in dessen Fensterkoordinaten, also
  relativ zum Ursprung dieses Monitors. Bei `gui` wählt `ScreenGrabber::selectMonitorAndCrop` den Monitor: mit
  `captureActiveMonitor=true` (blubs flameshot.ini) den unter dem Zeiger, sonst eine Monitorauswahl per Klick. Wir gaben
  absolute Koordinaten des virtuellen Desktops (DP-4 liegt bei +2160+1680), daher der Versatz. Fix: `flameshot screen
  --number N --edit --region WxH+X+Y --delay 300`: `--edit` macht daraus denselben Editor, `--number` wählt den Monitor
  vorab (`grabEntireDesktop` mit `preSelectedMonitor`, ohne `selectMonitorAndCrop`), also unabhängig von der
  flameshot.ini und vom Zeiger; die Datei wird nicht gelesen. Monitor = Screen des Fensters, Region relativ zu dessen
  Ursprung und auf ihn begrenzt (ein Fenster über zwei Monitore: der Teil auf seinem Screen). N = Index in
  `QGuiApplication::screens()`: Qt (xcb, `qxcbconnection_screens.cpp` in qt6-base 6.12) nimmt die RandR-Monitore in der
  Reihenfolge des X-Servers und stellt den primären an den Anfang; Lua bildet das aus Awesomes Screens nach (Index,
  primärer zuerst). Bei zwei Monitoren ist das in jeder Reihenfolge eindeutig. Test `palette_livefix.sh` (zwei Screens
  per `screen.fake_add`, argv des Fakes für Screen 1, Screen 2 und ein Fenster über der Grenze). Live zu prüfen: Bild
  zeigt exakt das Fenster auf DP-4 und DP-2.
- **Antwort-Panel von `?frage` verschoben.** Ursache: das Panel der Textaktionen verankerte sich wie bei `Mod+i` über
  der Mitte des fokussierten Fensters (`anchor`, mittig für eine typische Höhe von 520 px), die Palette steht aber
  mittig auf dem Screen bei etwa 18 % der Höhe. Der Screen war derselbe (beide `target_screen`), Lage und Breite nicht.
  Fix: `execute` gibt die Lage der Palette mit (`ctx.place`: Screen, x, y, Breite, auch nach dem Ziehen),
  `ask`/`explain_clipboard`/`text_action` reichen sie an `open_with` (`opts.place`), und das Panel steht auf diesem
  Screen, horizontal mittig wie die Palette, mit ihrer Oberkante (auf die Arbeitsfläche begrenzt). `Mod+i` bleibt
  beim Fenster.
- **Panels verschieben wie Fenster.** Neues Modul `awesome/dictate/drag.lua` für beide Panels: `Mod` + linke Taste
  überall (auch auf Knöpfen, ohne sie auszulösen: `awful.button` vergleicht Modifikatoren exakt; der Scrollbalken
  ignoriert Klicks mit Modifikator), ohne Modifikator an der Kopfzeile außer dem ✕, erst ab 4 px Bewegung. Über den
  `mousegrabber` nur während des Drückens, Lua blockiert nicht, der Fokus bleibt beim Zielfenster (Panel ist kein
  Client). Das Panel bleibt ganz auf der Arbeitsfläche des Screens unter dem Zeiger; Screenwechsel beim Ziehen macht
  den neuen Screen zum Panel-Screen (Breite und Grenzen von dort). Entscheidung: die gezogene Lage gilt bis zum
  Schließen und wird je Panel und Screen bis zum Neustart von Awesome im Speicher gemerkt (Abstand zur
  Arbeitsfläche, schwache Schlüssel), nie in einer Datei: wer ein Panel wegschiebt, will es meist beim nächsten Mal
  wieder dort, eine Datei bräuchte Regeln für geänderte Monitore. Für das Panel der Textaktionen gilt das nur für
  `Mod+i`; Sitzungen aus der Palette folgen der Palette (und merken sich ihr Ziehen nicht). Testnaht
  `forget_positions()`. Offen: Größe ändern mit `Mod` + rechter Taste (nicht umgesetzt); Eindruck am echten Desktop
  (picom, beide Monitore). Test `palette_livefix.sh` (Kopf, Schwelle, Mod, Rand, Screenwechsel, Klick ohne Bewegung
  führt aus, ✕ schließt, Lage gemerkt, Fokus), Screenshots `/home/data/stt-test/logs/palette-livefix-shots/`.
