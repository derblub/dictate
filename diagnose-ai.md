# Messschritt Diagnose-KI (Baustein 4, Aufgabe 10)

> **Archiv (W7, 21.11.2026):** Gemessen mit dem Katalog unter AwesomeWM/X11 (34 Proben, mit `xorg_log`) und dem
> damaligen Prompt („Arch Linux, X11, AwesomeWM“); seit W7 hat der Katalog 33 Proben, der Prompt nennt „Arch Linux,
> Wayland, niri“, und Fall 2 prüft die letzte Zeile des temp-loggers. Vor einer neuen Aussage zur Modellwahl neu messen.
> Gültig sind `README.md` und `PLAN-wayland.md`; der Inhalt hier bleibt unverändert.

Stand 05.11.2026. Frage: Bleibt Claude Sonnet 5.5 die Vorgabe für `diagnose.model`, oder reicht Claude Haiku 5.5?
Freigegeben waren 16 Live-Aufrufe (8 Fälle × 2 Modelle) und höchstens 4 Wiederholungen bei einem Fehler des
Messaufbaus.

## Aufbau

- **Messaufbau:** `src/diagnose/live_eval.rs`, nur unter `cfg(test)`. Darin acht synthetische Fälle nach echten
  Mustern, keine Zeile aus einem echten Log. Jeder Bericht entsteht wie im Produkt: Proben gegen Fakes und Fixtures, dann
  Filter (Schwärzen, Platzhalter), dann `Session`, dann `outgoing()`. Danach folgt eine Runde `ask_diagnosis` (Runde 1
  von 3) mit dem echten Prompt und Schema, `[ai]` auf Anthropic, Schlüssel nur über `ai.api_key_file`.
- **Tests:**
  - `cases_build_their_reports_through_the_product_path` läuft in der Unit-Suite ohne Netz. Er prüft, dass jeder Bericht
    die nötigen Belege enthält, kein Abschnitt fehlschlägt, keine persönlichen Daten und keine Geheimnisse drinstehen.
  - `live_diagnosis_against_claude` ist `#[ignore]` und startet nur mit `DICTATE_LIVE_DIAG=1`, nie in `check.sh`. Mit
    `=dry` schreibt er nur die Berichte. Aufruf steht im Modulkopf.
- **Rohdaten:** unter `/home/data/stt-test/logs/diagnose-ai/`, nicht im Repo: `dry/`, `run1/`, `run2-strict-5/`,
  `run2-strict-6/`. Je Lauf gibt es ein JSON mit Antwort, Belegauszügen, Tokens und Dauer sowie eine `summary.tsv`.
- **Bewertung:** Die Kriterien sind je Fall vor der Messung festgelegt (`hit`/`partial` in `live_eval.rs`, Tabelle
  unten). Bewertet ist von Hand. Der Schlüsselwort-Abgleich `grade_hint` dient nur als Hilfe.
  - „Belege gültig“: Rust hat nichts verworfen. Inhaltlich daneben liegende Belege stehen unter „Beobachtungen“.
  - „Prüfbefehle“: Zahl der gelieferten, dazu die von Rust verworfenen.

| # | Fall | Profil | Ja, wenn … | Teilweise, wenn … |
|---|---|---|---|---|
| 1 | Oops in `nvidia_isr` im pstore | Last crash | Kernel-Panik durch einen Oops im Interrupt-Handler des NVIDIA-Treibers | Panik ohne Treiber, oder „GPU/Treiber“ ohne Oops |
| 2 | stiller Hänger, pstore leer, temp-logger endet | Last crash | harter Hänger oder Stromverlust ohne Spur, Temperaturen normal, keine erfundene sichere Ursache | eine unbelegte Ursache als wahrscheinlich |
| 3 | OOM-Kill | Now | der OOM-Killer beendet `java`, weil RAM und Swap erschöpft sind | nur Speichermangel oder nur der Kill |
| 4 | Unit mit Exit-Code (`backup.service`) | Unit | rsync endet mit Code 23 (Teilübertragung) wegen Permission denied unter dem Home | nur eines von beiden |
| 5 | Container mit Segfault-Serie in `strlen` | Docker | Neustartschleife mit SIGSEGV in `strlen` aus `storage_open`, weil `storage.path` in der Config unbekannt ist | Absturzschleife ohne Bezug zur Config |
| 6 | CIFS-Hänger „has not responded in 180 seconds“ | Now | NAS antwortet nicht, Prozesse hängen im D-Zustand im CIFS-Code, daher hohe Last ohne CPU | nur eines davon |
| 7 | DNS kaputt (`~.` auf dem toten `wg0`) | Network | die Routing-Domain `~.` auf `wg0` schickt alle Anfragen an einen Server, den keine Route erreicht | `wg0`/VPN-DNS genannt, aber ohne `~.` oder ohne „unten“ |
| 8 | volle Platte | Now | `/` ist voll (100 %, 0 frei), daraus folgen die Schreibfehler und die gescheiterten Units | nur die Folgen oder eine andere Ursache |

## Befund 1: Die API lehnte das strikte Schema ab

Der erste Aufruf je Modell bekam **HTTP 400**: „The compiled grammar is too large, which would cause performance
issues. Simplify your tool schemas or reduce the number of strict tools.“

- **Ursache:** Das Schema der Prüfbefehle war die Vereinigung `anyOf` über 34 Objekte, je Probe eines. Die
  veröffentlichten Grenzen hielt es ein: 1 Vereinigung von höchstens 16, 0 optionale Felder von höchstens 24. Die
  Grammatik daraus überschreitet aber die interne Grenze, für die die API keine Zahl nennt.
- **Folge im Produkt:** `call_anthropic` lernt aus dem Wort „strict“ in der Absage `ToolCaps { strict: false }` und
  wiederholt ohne strict. Die Diagnose lief mit Claude also **nie strikt**.
  - Alle 16 Läufe der Messung 1 zeigen genau dieses Verhalten. Dazu kamen 2 abgewiesene Anfragen; sie haben keine
    Ausgabe und keine Tokens.
  - Ohne strict gab Haiku 5.5 in 3 von 8 Fällen eine unvollständige Antwort („The AI gave an incomplete diagnosis“).
    Beide Modelle lieferten viele ungültige Prüfbefehle: Haiku in 4 Fällen alle, Sonnet in den Fällen 5 und 6 alle.
- **Korrektur:** Die Prüfbefehle haben jetzt ein **flaches Schema**, `catalog::flat_schema`, aus derselben
  Katalogquelle.
  - Das Feld `probe` ist eine Aufzählung der 34 Namen. Jeder der 8 Parameter (`boot`, `lines`, `window_min`, `days`,
    `unit`, `since`, `container`, `target`) ist ein Pflichtfeld „Wert oder null“.
  - Ein Parameter mit verschiedenen Schemas je Probe bekommt den gemeinsamen Typ. Das betrifft nur `boot`.
  - Rust entfernt die `null`-Felder und prüft dann mit `parse_value` wie bisher. Ein fremdes Feld mit Wert gilt als
    verworfen.
  - Der Prompt sagt „null for the parameters it does not have“. Das gilt für alle drei Anbieter.
  - Tests:
    - `schema_is_strict_ready_and_uses_the_catalog` (neu gefasst)
    - `schema_stays_within_the_published_strict_limits` (≤ 16 Vereinigungen, 0 optionale Felder)
    - `flat_checks_with_null_parameters_parse_like_the_catalog` (jede Beispielprobe hin und zurück, fremdes Feld
      verworfen)
- **Bestätigung:** Gegen die API geprüft (siehe Kosten): Das alte Schema bekam 400, das flache 200. In den zwei
  Wiederholungsläufen mit strict lernte das Produkt nichts um (`caps unchanged`). Die Eingabe ist rund 3 000 Tokens
  kleiner.

## Ergebnis je Fall und Modell

Messung 1 lief ohne strict (Rückfall, siehe Befund 1). „W“ ist der Wiederholungslauf mit dem korrigierten,
strikten Schema. Tokens laut Server: Ausgabe einschließlich Denken.

| # | Modell | Ursache | Befunde | Belege verworfen | Prüfbefehle (verworfen) | Dauer s | Tokens ein/aus |
|---|---|---|---|---|---|---|---|
| 1 | Sonnet 5.5 | **ja** (Oops in `nvidia_isr`, Update am Vortag als Verdacht) | 4 | 2 | 2 (1) | 12,2 | 14 233 / 1 544 |
| 1 | Haiku 5.5 | **ja** | 5 | 1 | 4 (0) | 7,6 | 14 356 / 1 585 |
| 2 | Sonnet 5.5 | **ja** (Hänger, Strom oder Reset, Hitze und RAM ausgeschlossen) | 4 | 0 | 5 (0) | 14,5 | 12 898 / 1 679 |
| 2 | Haiku 5.5 | **ja** | 3 | 0 | 0 (5) | 6,9 | 13 021 / 1 331 |
| 3 | Sonnet 5.5 | **ja** (java ~30 GB, RAM und Swap voll) | 3 | 0 | 3 (0) | 15,1 | 9 460 / 1 698 |
| 3 | Haiku 5.5 | **ja** | 4 | 0 | 0 (3) | 5,1 | 9 583 / 985 |
| 4 | Sonnet 5.5 | **ja** (rsync 23 wegen Permission denied, NAS in Ordnung) | 3 | 0 | 4 (0) | 11,1 | 8 720 / 1 309 |
| 4 | Haiku 5.5 | **Fehler**: unvollständige Antwort | – | – | – | 4,0 | (nicht gemeldet) |
| 5 | Sonnet 5.5 | **ja** (strlen(NULL) in `storage_open`, `storage.path` unbekannt) | 3 | 0 | 0 (4) | 13,3 | 9 507 / 1 483 |
| 5 | Haiku 5.5 | **Fehler**: unvollständige Antwort | – | – | – | 5,0 | (nicht gemeldet) |
| 5 W | Haiku 5.5, strict | **ja** (Config-Bezug „medium“) | 3 | 0 | 1 (1) | 6,6 | 6 520 / 967 |
| 6 | Sonnet 5.5 | **ja** (NAS weg, D-Zustand, Last ohne CPU) | 2 | 0 | 0 (5) | 11,0 | 11 148 / 1 413 |
| 6 | Haiku 5.5 | **Fehler**: unvollständige Antwort | – | – | – | 7,2 | (nicht gemeldet) |
| 6 W | Sonnet 5.5, strict | **ja** | 3 | 0 | 0 (0) | 12,0 | 8 038 / 1 047 |
| 7 | Sonnet 5.5 | **ja** (`~.` auf dem toten `wg0`, „medium“) | 2 | 0 | 4 (0) | 11,3 | 8 465 / 1 426 |
| 7 | Haiku 5.5 | **teilweise** („no clear network fault“; `~.` nur als künftiges Risiko) | 2 | 0 | 2 (0) | 4,5 | 8 588 / 896 |
| 8 | Sonnet 5.5 | **ja** (`/` voll, Units als Folge, Docker als Verbraucher) | 3 | 0 | 5 (0) | 10,7 | 9 272 / 1 240 |
| 8 | Haiku 5.5 | **ja** | 4 | 0 | 0 (3) | 6,1 | 9 395 / 1 290 |

**Summe Messung 1:**

| Modell | Ursache getroffen | Befunde verworfen | Belege verworfen | Prüfbefehle geliefert (verworfen) | Dauer im Median | Eingabe je Lauf im Mittel | Ausgabe je Lauf im Mittel |
|---|---|---|---|---|---|---|---|
| Sonnet 5.5 | 8× ja | 0 | 2 | 23 (10) | 12,2 s | 10 500 | 1 470 |
| Haiku 5.5 | 4× ja, 1× teilweise, 3× Fehler | 1 | 1 | 6 (11) | 6,1 s | 6 900 | 760 |

Bei Haiku zählen für Eingabe und Ausgabe nur die 5 Antworten.

### Beobachtungen

- **Prüfbefehle:** Die von Sonnet sind fast immer sinnvoll: die Journale von Boot −2 auf ein wiederkehrendes Muster,
  `top_procs`/`oom` nach dem Kill, `timers`, `net_mounts`, `docker_df` und die Journale der gescheiterten Units bei
  voller Platte, `wg-quick@wg0` und `net_reach gateway` bei DNS.
  - Haiku schlug in Fall 1 `kernel_errors boot=-1` vor, obwohl der Bericht genau diesen Abschnitt schon hat (der Prompt
    verbietet Wiederholungen), dazu eine Unit `nvidia`, die es nicht gibt.
  - Unit-Namen prüft Rust erst beim Ausführen gegen die echte Liste (`unit=nvidia`, `wg-quick@wg0`, `user@1000`). Ein
    erfundener Name scheitert dann mit „no unit matches“; das ist so gewollt.
  - Sonnet in Fall 6 mit strict: keine Prüfbefehle, obwohl `net_reach nas` und `net_mounts` nahelägen. Der Befund sagt
    selbst „no network probe has run yet“.
- **Belege:** Rust verwarf kaum etwas: Sonnet 2, Haiku 1. Inhaltlich lagen einzelne Belege beider Modelle um eine Zeile
  daneben, zum Beispiel `S1 4-4` für Boot 0 statt −1 oder eine Zeile von tailscaled statt des USB-Resets. Das Panel
  zeigt den Beleg, der User sieht das. Eine Messung der Zählweise wäre ein eigener Schritt (offen).
- **Ohne Beschwerde des Users:** Der Prompt kennt nur das Profil, nicht das Symptom. Haiku schrieb in Fall 7 „The
  user's specific problem is not described“ und schwächte den richtigen Befund ab; Sonnet nannte ihn trotzdem. Ein
  optionales Freitextfeld „Was ist los?“ wäre eine Produktentscheidung (offen, nicht gebaut).
- **Aus den Berichten, unabhängig vom Modell (Funde im Filter, offen):**
  - Der Kopf von `docker_state` mit nur `container=dataroom-worker-1` wird zu `[REDACTED:password]`.
  - In `unit_show` gelten `ExecMainStartTimestamp=Thu …` und `FragmentPath=/etc/systemd/system/backup…` als
    `high_entropy_token`.
  - Wiederholte Mehrzeilenblöcke in `docker_logs` ergeben neun gleiche Zeilen „… 9× more of the message above omitted“.
  - Pfade unter `/home` werden ganz zu `path-N`. In Fall 4 ging damit der Hinweis `pgdata` verloren; das Kriterium
    wurde vor dem Lauf darauf angepasst.

## Kosten

Preise laut Skill `claude-api` (Stand 06.10.2026, je Million Tokens): Sonnet 5.5 $2 ein / $10 aus, Haiku 5.5 $0,10 ein /
$0,50 aus (bis 100 000 Tokens Eingabe).

| Posten | Aufrufe | Kosten |
|---|---|---|
| Messung 1 Sonnet 5.5 | 8 | $0,285 |
| Messung 1 Haiku 5.5 | 8 | $0,009, mit den 3 Fehlläufen (Tokens nicht gemeldet, geschätzt) etwa $0,014 |
| Wiederholung Sonnet 5.5, strict | 1 | $0,027 |
| Wiederholung Haiku 5.5, strict | 1 | $0,001 |
| Prüfung des Schemas (Haiku, 16 Tokens Ausgabe) | 2 | < $0,001 |
| Rückfall von strict im Produkt | 2 | $0 (keine Ausgabe, keine Tokens) |
| **Summe** | **22 HTTP-Anfragen** | **etwa $0,33** |

Abgerechnet wurden 19 der 22 Anfragen. Die 16 freigegebenen Läufe waren dabei, dazu 4 Wiederholungen wegen des
Schemas: 2 Prüfungen und 2 Läufe mit strict. Damit ist das Budget für Wiederholungen ausgeschöpft.

Eine Diagnose kostet etwa 3,5 ct mit Sonnet und 0,1 ct mit Haiku. Beides ist für ein Werkzeug, das einige Male im Monat
läuft, vernachlässigbar.

## Empfehlung

**Sonnet 5.5 bleibt Vorgabe.** Es traf in allen 8 Fällen die Ursache, wählte vorsichtige Sicherheiten (Fall 2 „medium“,
keine erfundene Ursache) und schlug die brauchbareren Prüfbefehle vor. Auch ohne strict blieb seine Antwort
vollständig.

Haiku 5.5 ist halb so schnell (6 s statt 12 s) und 30-mal billiger. In Messung 1 ohne strict war es aber nicht
verlässlich: 3 von 8 Antworten unbrauchbar, ein Fall nur teilweise, kaum gültige Prüfbefehle. Mit dem strikten Schema
war der einzige Wiederholungslauf (Fall 5) richtig.

Ob Haiku mit strict für alle 8 Fälle reicht, ist nicht gemessen: 8 weitere Aufrufe, unter 1 ct. Das entscheidet Daniel.
Der Unterschied im Preis spricht nicht gegen Sonnet.

## Nachtrag 21.11.2026: Haiku 5.5 mit strict über alle 8 Fälle

Stand nach W7 (33 Proben, Prompt „Arch Linux, Wayland, niri“, Fall 2 mit der letzten Zeile des temp-loggers), strict
ist inzwischen der normale Weg (`caps unchanged` in allen Läufen). 8 Aufrufe, Rohdaten unter
`/home/data/stt-test/logs/diagnose-ai/run3-haiku-strict/` (nicht im Repo). Bewertet von Hand gegen die Kriterien aus
`live_eval.rs`; die Belege mit Zeilennummern wurden gegen den Bericht gelesen.

| # | Ursache | Befunde | Belege verworfen | Prüfbefehle (verworfen) | Dauer s | Tokens ein/aus |
|---|---|---|---|---|---|---|
| 1 | **ja** (Oops in `nvidia_isr`, Update am Vortag als Verdacht; Belege S4 Z. 2, 6, 26) | 4 | 0 | 4 (0) | 9,7 | 11 195 / 1 672 |
| 2 | **ja**, mit Abstrich (Hänger oder Stromverlust „medium“; USB-Resets als mögliche Ursache ebenfalls „medium“) | 6 | 0 | 4 (1) | 9,2 | 9 788 / 1 795 |
| 3 | **ja** (java ~30 GB, RAM und Swap voll) | 4 | 0 | 4 (0) | 7,5 | 6 636 / 1 341 |
| 4 | **ja** (rsync 23 wegen Permission denied unter `pgdata`) | 2 | 0 | 1 (1) | 5,5 | 5 921 / 984 |
| 5 | **ja** (strlen(NULL) in `storage_open`, `storage.path` unbekannt) | 2 | 0 | 4 (1) | 7,0 | 6 310 / 1 406 |
| 6 | **ja** (NAS antwortet nicht, D-Zustand, Last ohne CPU) | 4 | 1 | 4 (0) | 7,6 | 8 444 / 1 451 |
| 7 | **ja** (`~.` auf dem toten `wg0`, „medium“; im Lauf ohne strict nur teilweise) | 3 | 0 | 3 (1) | 5,8 | 5 591 / 1 141 |
| 8 | **ja** (`/` voll, Units als Folge) | 4 | 0 | 3 (2) | 7,3 | 6 448 / 1 441 |

- **Summe:** 8 von 8 getroffen, keine unvollständige Antwort, 1 Beleg und 6 von 27 Prüfbefehlen verworfen, Median 7,4 s.
- **Kosten:** 60 333 Tokens ein, 11 231 aus, mit den Preisen oben etwa $0,012 (rund 0,15 ct je Diagnose; Sonnet 5.5 in
  Messung 1 rund 3,6 ct).
- Ein erster Versuch am selben Tag scheiterte an HTTP 401 (der Schlüssel in `~/.config/anthropic/api-key` war ungültig,
  ersetzt durch den gültigen aus `console-api-key`); er kostete nichts.
- **Entscheidung Daniel (21.11.2026): Sonnet 5.5 bleibt Vorgabe** für `diagnose.model`. Beide Modelle treffen mit strict
  alle 8 Fälle; eine Diagnose läuft selten, die Kosten sind absolut gering, und Haiku spekuliert etwas mehr (Fall 2).
  Haiku bleibt im Einstellungsfenster wählbar. Acht synthetische Fälle zeigen keinen Unterschied in der Trefferquote.
