# Plan: Baustein 2 „Textaktionen auf Markierung“

> **Archiv (W7, 21.11.2026):** Beschreibt den Stand unter AwesomeWM/X11; AwesomeWM, die X11-Wege und die CLIs sind
> entfernt, dictate läuft unter niri (Wayland). Gültig sind `README.md` und `PLAN-wayland.md`; der Inhalt hier bleibt
> unverändert.

Arbeitsplan für `ROADMAP.md` Baustein 2. Vorgabe des Users: keine Abkürzungen, immer die fachlich richtige Lösung.

## Stand (23.10.2026)

| # | Aufgabe | Stand |
|---|---|---|
| 1 | Config `[actions]`, Katalog | fertig (8dd80db) |
| – | KI-Modi: Anrede, Modell je Modus, Thinking je Modell, `INPUT_GUARD` | fertig (de3fd92, 0c73114, b27502d, 358b43a) |
| 2 | Geheimnisfilter, `ai::ask` | fertig (c0f02e3, 5a8f5a9, 9d06f40) |
| 2b | Geheimnisfilter: EFF-Wortliste für Passphrasen ohne Ziffer (Regel 8c, `src/actions/wordlist.rs`) | fertig (7180fb7) |
| 3 | Protokoll und Daemon-Speicher | fertig (9d06f40 Vorab Passwort-Regel, 4dc3db4) |
| 4 | CLI `dictate act` | fertig (bf1bf93) |
| 5 | Awesome-Panel, Sperre löscht die Markierung | fertig (21c9fc5) |
| 7 | Streaming ins Panel (Pflicht) | fertig (b6ff9f6, 26bdbd6 eigener Socket) |
| 6 | Einstellungsfenster und Doku | fertig (078ce64 Seite Actions, Doku siehe unten) |
| 8 | Aktion Refine prompt (`p`, Ersetzen) und Diktat-Modus `prompt`, ein Prompt `PROMPT_REFINE`; Live-A/B Haiku 5.5 | fertig (siehe `SPEC-v2.md`, Modus `prompt`) |
| 9 | Panel nach `UX-actions.md`: Maus, Gruppen, Primärknopf, Totzeit, Markdown, Wortdiff, Copy je Codeblock, Leistensymbol, Mod+Ctrl+e stoppt Replace | fertig (Stand in `UX-actions.md`) |

## Antworten des Users (22.10.2026)

- Kürzel **Mod+i**.
- Erklärungen und Fehleranalysen in der **eingestellten Sprache** (`actions.language`, leer = globales `language`).
- Geheimnisse: **Cloud sperren, lokal erlauben** (`secrets = "local_only"`).
- Streaming ist Pflicht, nicht optional.

## Entscheidungen

- **Erweiterung dieses Binaries** (Unterbefehl `dictate act …` + Socket-Befehle `act_*`). Wiederverwendet: Daemon,
  Config (`Config::save`/toml_edit), `ai.rs`, Socket 0700, `tests/mock_http`, `tests/daemon_common`. KI-Aufrufe auf dem
  Verbindungs-Thread wie `AiTest` in `execute()` (daemon.rs); der Kern blockiert nie.
- **PRIMARY liest das CLI**, das Awesome startet (der Dienst hat kein DISPLAY): erst `xclip -o -selection primary -t
  TARGETS`, dann `-t UTF8_STRING` (streng UTF-8, ungültig = Fehler) oder, wenn der Besitzer nur `STRING` bietet, `-t
  STRING` als ISO 8859-1; ohne `TARGETS`-Antwort direkt `UTF8_STRING`. (Statt „Fallback ohne `-t`“: xclip 0.13 fällt
  bei `-t UTF8_STRING` selbst still auf `STRING` zurück und lieferte Latin-1-Bytes als vermeintliches UTF-8.)
  „target … not available“ = nichts markiert. Frist 2 s für alle Aufrufe zusammen mit Kill, Grenze 30000 Zeichen
  (`config::ACTIONS_MAX_CHARS`, gelesen höchstens ×4 Byte; `actions.max_chars` prüft der Daemon),
  `DICTATE_XCLIP` ersetzt das Programm in Tests.
  Der Text geht per Socket an den Daemon (`act_grab`), nie per argv. Der Daemon hält eine Sitzung, TTL 300 s. CLIPBOARD
  wird für die Markierung nie gelesen, kein Fallback. (Einzige Ausnahme im Projekt: `dictate paste` sichert CLIPBOARD
  vor dem Einfügen und legt sie danach unverändert zurück. Der Inhalt bleibt im Speicher dieses Prozesses bzw. seines
  Halteprozesses, wird nie geloggt, nie an Daemon oder KI geschickt und nie als Markierung verwendet. Begründung: Der
  User hat entschieden, außerhalb von Terminals einzufügen statt zu tippen (schneller, keine verlorenen Zeichen,
  Zeilenumbrüche ohne Senden in Chat-Feldern); ohne Sichern verlöre er dabei seine Zwischenablage.) Leer → „Nothing selected“.
- **Menü = eigenes Awesome-Panel** `awesome/dictate/actions.lua` mit `awful.keygrabber` (Fokus bleibt im Zielfenster),
  Design wie das Overlay (`DESIGN.md`: Ink, Teal, Pixel-Wait). Kein rofi.
- **Replace** über die bestehende Tipp-Warteschlange (`type_output`/`pump_typing`). Nur wenn die Aktion
  `output="replace"` hat, derselbe Client noch fokussiert ist und WM_CLASS nicht in `actions.terminal_classes` steht
  (Daemon liefert `replace_ok` aus `--app`). Steuerzeichen außer `\n`/`\t` entfernt der Daemon.
  Seit 23.10.2026 fügt die Warteschlange ein statt zu tippen (`dictate paste`, ein Auftrag, Fortschritt am Ende,
  `Mod+Ctrl+e` schickt SIGTERM und stellt die Zwischenablage sofort wieder her); getippt wird nur noch im Rückfall
  (Zwischenablage nicht sicherbar) oder mit `setup{ output = "type" }`.
- **Copy**: `dictate act copy --id X` holt das Ergebnis vom Daemon und schreibt es per stdin an
  `xclip -selection clipboard -i`. Der von xclip geforkte Besitzer-Prozess bleibt gewollt im Hintergrund; xclip bekommt
  darum stdout `/dev/null` und stderr in eine sofort gelöschte Datei (kein Pipe, das `easy_async` offen hielte), der
  direkte Kindprozess wird immer eingesammelt.
- **Streaming** (Aufgabe 7): freie Aktionen (`ask`) erscheinen live im Panel, Esc beendet das CLI per PID und ruft
  `act cancel`. KI-Modi streamen nicht: ihr Ersatztext wird erst als Ganzes geprüft (abgeschnitten = Fehler, Länge
  gegen die Eingabe, Präambel/Anführungszeichen erst am Ende erkennbar, Umbrüche je nach Gesamtzahl eingeebnet);
  gestreamte Stücke wären nicht der Endtext. Dort bleibt „Working… Esc cancels“. Das Diktat streamt nie.
- **Kürzel** über `dictate.keys(modkey)`, `rc.lua` bleibt unverändert, abschaltbar mit `setup{ actions_key = false }`.
- **Keine Logs der Texte**, auch nicht mit `DICTATE_DEBUG`; nur Aktion, Zeichenzahl, Dauer, Geheimnis-Arten. Nie in
  `history.rs`. Speicher bei `act_clear`, neuem grab und Sperre leeren und überschreiben.
- **Schutzrahmen** `INPUT_GUARD` (config.rs) steht vorn in jedem Aktions-Prompt. Antworten werden nie ausgeführt; Lua
  ohne `with_shell`/`io.popen`/`os.execute`.
- **Sperre** (Entscheidung des Users 23.10.2026): beim Sperren schließt das Panel und `act clear` löscht die
  Markierung. Auslöser in Awesome (`awesome/dictate/lock.lua`, lgi/Gio asynchron) über logind auf dem System-Bus:
  `LockedHint` der eigenen Sitzung (`GetSession($XDG_SESSION_ID)`), `Lock` und `PrepareForSleep(true)`. xss-lock 0.4
  setzt `LockedHint`, sobald es den Locker startet, also bei Mod+Ctrl+l (`xset s activate`), Leerlauf, Suspend und
  `loginctl lock-session`; ohne Änderung an Dotfiles. In Awesome statt im Daemon, weil das Panel den Tastatur-Grab
  freigeben muss, damit xsecurelock greifen kann. Nicht erfasst: `rofi-power` › Lock startet `lock-screen` direkt
  (an xss-lock vorbei); Vorschlag: dort `loginctl lock-session`. Zusätzlich schließt das Panel nach 300 s ohne Taste.
- **Cloud-Vorschau**: das Panel zeigt vor einer Cloud-Anfrage Host, Modell, Zeichenzahl und die ersten Zeilen; die Wahl
  einer Aktion gilt als Bestätigung.

## Protokoll

| Anfrage | Antwort |
|---|---|
| `{"cmd":"act_grab","text":"…","app":"Alacritty"}` | `{"ok":true,"id":"7","chars":342,"lines":7,"target":{"host":"api.anthropic.com","model":"…","cloud":true},"secret":["private_key"],"blocked":false,"replace_ok":false,"actions":[{"key":"e","name":"explain","label":"Explain","output":"answer","allowed":true},…]}` |
| `{"cmd":"act_run","id":"7","action":"explain"}` | `{"ok":true,"text":"…","output":"answer","truncated":false}` (ersetzende Aktionen mit Markierung zusätzlich `"diff":[["=","…"],["-","…"],["+","…"]]`) oder Fehler (unknown or expired selection, blocked (secret), AI server changed, actions disabled, KI-Fehler) |
| `{"cmd":"act_cancel","id":"7"}` | `{"ok":true}` |
| `{"cmd":"act_result","id":"7"}` | `{"ok":true,"text":"…"}` (für Copy) |
| `{"cmd":"act_clear"}` | `{"ok":true}` |

Vor der Endantwort sendet `act_run` (nur freie Aktionen) Zwischenzeilen `{"delta":"…"}`: das erste Stück sofort,
danach höchstens eine Zeile je 50 ms, früher ab 4 KB; jedes Stück endet an einer Graphem-Grenze. Bei Erfolg ist der
Endtext genau die Verkettung, bei Fehler sind die Stücke zu verwerfen. Scheitert das Schreiben (Client weg), bricht der
Daemon den KI-Aufruf ab.

CLI `--format awesome` (Tab-getrennt, alle Textfelder mit `protocol::esc`, Wahrheitswerte 0|1):
- grab: `grab\t<id>\t<chars>\t<lines>\t<cloud 0|1>\t<host>\t<model>\t<secret kinds,|->\t<replace_ok 0|1>\t<preview>`,
  je Aktion `action\t<key>\t<name>\t<label>\t<output>\t<allowed 0|1>\t<model>` (Modell dieser Aktion für die
  Cloud-Vorschau), dann `end`. Vorschau max. 6 Zeilen / 400 Zeichen, Rand ohne Leerraum, ohne Steuerzeichen außer
  `\n`/`\t`.
- run: beliebig viele `delta\t<text>` (Zeilen ohne `"ok"` vor der Antwort; andere solche Zeilen überliest das CLI),
  dann `result\t<output>\t<truncated 0|1>\t<text>`, `cancelled` (Abbruch) oder `error\t<msg>`.
- run (ersetzende Aktionen mit Markierung): direkt vor `result` je Diff-Stück `diff\t<op>\t<text>` (`=`, `-`, `+`;
  ohne `+` ergibt die Markierung, ohne `-` das Ergebnis). Kein Diff bei mehr als 50 % Änderung oder `truncated`.
- copy: `copied\t<chars>` (der Text erscheint nicht auf stdout); `--block N` kopiert nur den N-ten Codeblock (Regeln
  in `src/actions/markdown.rs`, gleich in `awesome/dictate/markdown.lua`), sonst `error\tNo code block N`; cancel,
  clear: `ok`.
- Fehler aller Unterbefehle: `error\t<msg>`, Exit 1; Daemon nicht erreichbar: `error\tThe dictate daemon is not
  running`, Exit 2 (im JSON-Format wie die übrigen Befehle nur stderr). JSON: Antwortzeile des Daemons unverändert,
  bei grab ergänzt um `"preview"`, bei copy `{"ok":true,"chars":N}`.
- Frist für run: `actions.timeout_ms` des laufenden Daemons (per `get_config` auf derselben Verbindung, ohne Angabe
  120 s) + 5 s; danach `act_cancel` und `error\tNo answer from the daemon within N s`.

Befehlspalette (PLAN-palette.md, Aufgabe 9): `act_grab` mit `"source":"question"|"clipboard"` (Vorgabe `selection`;
CLI `act grab --source question` liest die Frage von stdin, `--source clipboard` liest CLIPBOARD). Solche Sitzungen
bieten genau eine Aktion (`ask` mit festem Rahmen `ASK_FRAME` bzw. `explain`), hängen an `palette.enabled` statt
`features.actions`, nutzen `palette.model` und sperren bei einem Geheimnis nur die Cloud (`actions.secrets = "block"`
gilt nur für Markierungen); `replace_ok` ist immer falsch, die Antwort enthält `"source"`. `act_run`, `act_cancel`,
`act_result` und `act_clear` gelten unverändert.

`check_size`: `act_grab` bis 256 KB; `actions.max_chars` begrenzt inhaltlich.

## Aufgaben

### 3. Protokoll und Daemon-Speicher
Dateien: `src/protocol.rs`, `src/daemon.rs` (execute-Zweige, `Arc<Mutex<ActStore>>`, `check_size`-Ausnahme, Config-Kopie
analog `Msg::AiCfg`), neu `src/actions/store.rs`.
- grab: Feature aus → „Text actions are off (Settings › Actions)“; `chars > max_chars` → Fehler; Filter; Ziel
  auflösen. Geheimnis und (Cloud oder `secrets=block`) → alles gesperrt, Text nicht gespeichert. `replace_ok` = App nicht
  in `terminal_classes`. Neue Sitzung ersetzt die alte, TTL 300 s.
- run: Ziel neu auflösen, Host/Cloud anders → „AI server changed, select again“; gesperrt → Fehler; Aktionen mit
  `mode` über `rewrite_as` (Modell nach `ActionsCfg::model_for`), sonst `ask`; Ergebnis speichern; höchstens ein Aufruf
  je Sitzung (ein zweiter bricht den ersten ab).
- Tests `tests/daemon_actions.rs`: grab+run lokal (Mock-Body enthält Text und Schutzrahmen); Geheimnis+Cloud → blocked,
  Mock bekommt 0 Anfragen; lokal+`local_only` erlaubt, `block` gesperrt; Feature aus; abgelaufene id; Zielwechsel per
  `set_config`; cancel < 500 ms bei 3-s-Mock; Größe; Terminal → `replace_ok=false`; Modus `clean` über `rewrite_as`;
  `status` antwortet während eines langsamen run.

### 4. CLI `dictate act`
Dateien: `src/main.rs` (`act grab [--app]`, `run --id --action`, `cancel --id`, `copy --id`, `clear`,
`--format json|awesome`), `src/client.rs` (`act_*_at`; Lese-Timeout für run = `actions.timeout_ms` + 5 s), neu
`src/actions/selection.rs` (`read_primary()`, `write_clipboard()`; argv, keine Shell).
Akzeptanz: Lesen nur `-selection primary`; leer → `error\tNothing selected`, Exit 1; Daemon fehlt → Exit 2;
Maskierung korrekt; copy schreibt genau das Ergebnis.
Tests `tests/act_cli.rs` mit Fake-xclip (protokolliert argv, `-o` gibt Text aus, `-i` speichert stdin); `#[ignore]` mit
echtem xclip unter Xvfb.

### 5. Awesome-Panel `actions.lua` + Xephyr-Tests
Dateien: neu `awesome/dictate/actions.lua`; `awesome/dictate/init.lua` (Helfer über `M._shared`, Kürzel Mod+i in
`M.keys`, Gruppe Launch, `once()` gegen Autorepeat); `awesome/test/fake-dictate` (`act`, calls.log, Variante „secret“);
`awesome/test/run.sh`.
Ablauf (nur `easy_async`/`with_line_callback`, argv-Listen): Kürzel merkt Client, spawnt `act grab --app <class>
--format awesome` → Panel mittig mit Kopf „Selection · N chars · → host (model)“ oder „local“, Vorschau grau-400 Mono,
Geheimnis-Warnung red-500, Aktionsliste (gesperrte gedimmt) → Taste → `act run` mit Pixel-Wait; Esc → kill PID,
`act cancel`, `act clear` → Ergebnis mit Scrollen, „Enter copy · r replace · Esc close“ → Enter/c `act copy`; r nur bei
replace + `replace_ok` + gleichem Client → jedes Schließen `act clear`. Zweites Mod+i schließt. `_debug()` um
`act_state`, `act_id`, `act_actions`, `act_result_len` erweitern.
Xephyr-Tests: grab mit WM_CLASS; explain + copy; replace tippt in entry.txt; Fokuswechsel → nichts; `replace_ok=0` →
nichts; secret → kein run; Esc → cancel + clear; Daemon weg → Hinweis; grep auf with_shell/io.popen/os.execute leer.

### 7. Streaming ins Panel (fertig)
`ai.rs`: `AskOpts::on_delta` schaltet `stream: true` ein. Claude SSE (`content_block_delta`/`text_delta`; Thinking-,
Signatur- und andere Deltas ignoriert; `message_delta.stop_reason`; `error`-Event = Fehler; `ping`), OpenAI SSE
(`choices[0].delta.content`, `finish_reason`, `[DONE]`), Ollama NDJSON (`message.content`, `done`, `done_reason`).
Zerlegung auf Bytes, dekodiert werden ganze Zeilen (UTF-8 und Events über Stückgrenzen); `\n`, `\r\n`, `\r`.
`AnswerStream` säubert die Stücke so, dass ihre Verkettung genau `clean_answer` ist (Steuerzeichen je Zeichen,
Leerzeilen vorn und Leerraum hinten zurückgehalten, letzte Graphem-Gruppe zurückgehalten). Thinking- und
`max_completion_tokens`-Rückfall gelten gleich (Absage als HTTP-Status vor dem Stream). Antwortet ein Server mit
`application/json`, wird das Objekt wie bisher gelesen, ohne Deltas.
Zeiten und Abbruch: gestreamt über `src/http.rs` (anfangs `src/pipeline/http_stream.rs`) auf eigenem Socket (HTTP/1.1,
TLS über rustls mit denselben Wurzeln wie vorher ureq), ohne Lese-Thread. Socket-Fristen 50 ms, vor jedem Lesen und Schreiben werden Abbruch,
Gesamtgrenze `timeout_ms` (ab Aufruf, gilt immer) und Leerlauf `idle_ms` (Vorgabe 30 s, ab Antwortkopf, jedes Byte
zählt, auch `ping`) geprüft; beim Verlassen schließt die Verbindung sofort (auch vor dem Kopf und bei hängendem
Server). Warum nicht ureq 2: `timeout_read` gilt nur ohne Gesamtfrist (`stream.rs` `connect_socket`,
`DeadlineStream::fill_buf`), der Socket ist `pub(crate)` (nur über einen `TlsConnector` erreichbar, nie bei
`http://`), und kurze Fristen mit Wiederholung brechen den Chunk-Zerleger (`chunked/decoder.rs` `read_chunk_size`).
Erledigt (Review `review-http-stream.md`, 23.10.): auch der nicht gestreamte Pfad (`call`: Diktat-Modi über
`rewrite_as`/`rewrite_with`, `/models`, `AiTest`) und die Modell-Downloads laufen über denselben Client; Abbruch lässt
keinen Thread und keine Verbindung mehr nachlaufen (nur die Namensauflösung darf in ihrem Hilfsthread zu Ende laufen,
ohne Socket). ureq ist entfernt. Befunde: Prüfung nach jedem `read_tls`/`write_tls` (M1); Connect nicht blockierend
mit `poll(2)` und Happy Eyeballs (RFC 8305, 250 ms), DNS abbrechbar (M2); TLS-Ende ohne `close_notify` bei
Close-Rahmung = abgeschnitten (N1); Kopf über `httparse`, Rahmung streng nach RFC 9112 §6.3 (Content-Length nur
dezimal und gleich, TE nur `chunked`, 1xx überspringen, 101 Fehler, 204/304 ohne Körper) (N2); lineare
Kopfende-Suche (N3); Chunked streng, Trailer zusammen ≤ 16 KiB, Rahmung ≤ 64 KiB + 8 × Nutzdaten (N4); `error_body`
reicht Abbruch/Frist weiter, 401/403 ohne Lesen (N5); Kopfzeilen nur `tchar`/sichtbares ASCII (N6). Downloads: Abbruch
sofort statt nach bis zu 30 s Lesefrist, Weiterleitungen über `get_following_https`.
Tests: `ai.rs` (Zerlegung byteweise, Verkettung == `clean_answer` bei zufälligen Schnitten), `tests/ai_http.rs`
(drei Protokolle mit 7-Byte-Stücken, truncated, Rückfälle, Fehler-Event, Ablehnung, Stream ohne Ende, Server ohne
Streaming, Gesamt- und Leerlaufgrenze, Abbruch < 200 ms schließt die Verbindung, hängender Server nach Kopf und vor
Kopf wird bei Abbruch < 200 ms bzw. nach `idle_ms` geschlossen, `#[ignore]` live mit Haiku),
`tests/daemon_actions.rs` (Drosselung, Endtext == Verkettung, Modi ohne Stream, Subscriber, Fehler mitten im Stream,
Client weg bricht ab), `tests/act_cli.rs` (delta-Zeilen vom echten Daemon). Live (Haiku 5.5, eigener TLS-Pfad): 66 Deltas,
erstes nach 1,5 s, fertig nach 3,2 s.

### 6. Einstellungsfenster + Doku
Dateien: neu `src/gui/pages/actions.rs`, `src/gui/pages/mod.rs`, `src/gui/mod.rs` (`Page::Actions` nach AI),
`src/gui/pages/general.rs` (Kürzel-Tabelle), `src/gui/dto.rs`, `tests/gui/mock_daemon.py`, README.md, ROADMAP.md,
CLAUDE.md.
Seite: Feature-Schalter, Server-Hinweis wie `pages/ai.rs`, Model override, Timeout, Max chars, Secrets („Block for
cloud, allow local“/„Always block“), Answer language, „Offer AI modes“, Terminal classes, Prompt-Editor je Aktion,
Kürzel-Zeile. Englisch, `DESIGN.md`. Prüfen per `tests/gui/run-gui.sh` und `tests/gui/shots.sh`.
Umgesetzt (23.10.2026), Abweichungen und Ergänzungen:
- Gruppen Server, Privacy, Answers, Replace, Prompts, Shortcut. „Max chars“ heißt im Fenster „Longest selection“.
  Server-Hinweis zeigt Host (`dto::Ai::host`, dieselbe Auflösung wie der Daemon) und Local/Cloud, dazu „Open the AI
  page“; Aktionen nutzen den Server auch bei `features.ai = false`.
- Privacy zeigt die erkannten Arten aus `secrets::KINDS`/`describe` und drei Sätze: nur PRIMARY, keine Logs, Sperre
  löscht.
- Prompt-Editor: fehlt ein Name in `actions.prompts` (Tabelle ersetzt die Vorgabe ganz), zeigt das Fenster die Vorgabe
  und trägt sie erst beim Bearbeiten ein. „Reset to default“ stellt den gespeicherten Zustand her (Vorgabetext bzw.
  ohne Eintrag), damit die Seite nicht grundlos „geändert“ ist. Schutzrahmen-Hinweis aufklappbar (`ai::guard_note`).
- Feldfehler aus Listen: der Daemon meldet `actions.terminal_classes[7]`; `App::err_under`/`err_index` zeigen die
  Meldung am Feld und färben den Eintrag rot (`widgets::string_list_marked`). Derselbe Fehler bestand bei
  `profiles[i].match_class[j]` (Meldung erschien nur unten), mit behoben.
- „↔“ in „Translate DE ↔ EN“ fehlte in Google Sans Flex: DejaVu Sans als Rückfallschrift (`theme::install`).
- Mock: `[actions]` (Prompt-Tabelle ohne `explain_code`, eigener `explain`), Validierung wie `Config::validate`,
  `MOCK_CLOUD=1` für den Cloud-Hinweis. Screenshots `/home/data/stt-test/logs/act6-*.png` (Seite, Feldfehler,
  Cloud, General, alle Seiten `act6-all-*`).
- Doku: README (Bedienung, Sicherheit, CLI, Config-Referenz `[actions]`), ROADMAP (Baustein 2), SPEC-v2 (Protokoll,
  Feature-Schalter, Seite), CLAUDE.md (Stand, Kürzel, Offen, Fallen).

## Reihenfolge

3 → 4 → 5 → 7 → 6, nacheinander (gemeinsamer Arbeitsbaum, sonst brechen sich die Builds gegenseitig). Danach
Installation und Live-Test durch den User; das Feature ist standardmäßig aus.
