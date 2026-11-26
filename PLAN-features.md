# Plan: kleine Funktionen und freie Modellwahl (Bausteine 7 und 8)

Stand 26.11.2026: **nur Plan, nichts gebaut.** Dieser Plan gehört zu `ROADMAP.md` (Bausteine 7 und 8). Er hält fest,
was Daniel am 25./26.11. wünschte: das GlobalShortcuts-Portal prüfen (Ergebnis: Abschnitt 1), vier kleine
Funktionen planen (Abschnitt 2) und eine freie Modellwahl über mehrere Erkennungs-Engines samt CPU-Weg planen
(Abschnitt 3). Aufwandsangaben sind grobe Schätzungen von Claude für einen Arbeitstag mit Prüfläufen; sie sind kein
Maßstab für die Entscheidung (Arbeitsweise: fachlich richtige Lösung, nicht die leichtere).

Kennzeichnung wie in `PLAN-wayland.md`: **gemessen** (selbst gelaufen), **gelesen** (Quelltext oder Beschreibung
Dritter, nicht ausgeführt), **abgeleitet** (Schluss daraus), **ungemessen** (offen).

## 1. GlobalShortcuts-Portal auf blub: Messung am 26.11.2026

**Ergebnis: geht auf blub heute nicht.** Der Wunsch war, die Gruppe `input` (liest `/dev/input/event*`, also jeden
Tastendruck) durch das Portal zu ersetzen. Das ist mit niri 26.04 nicht möglich; die `input`-Gruppe bleibt.

| Punkt | Befund | Art |
|---|---|---|
| Umgebung | niri 26.04 (`8ed0da4`), `xdg-desktop-portal` 1.22.1, `-gnome` 51.0, `-gtk` 1.15.3; `niri-portals.conf`: `default=gnome;gtk` | gemessen |
| Frontend | `org.freedesktop.portal.GlobalShortcuts`, Version 2, ist auf dem Session-Bus exportiert (`busctl --user introspect`) | gemessen |
| Backend | `org.freedesktop.impl.portal.desktop.gnome` läuft und exportiert `org.freedesktop.impl.portal.GlobalShortcuts` | gemessen |
| App-ID | Ohne App-ID antwortet `CreateSession` mit `NotAllowed: An app id is required`. Programme außerhalb einer Sandbox brauchen `org.freedesktop.host.portal.Registry.Register(app_id)` **und** eine passende `<app_id>.desktop` in den Anwendungsordnern (sonst „App info not found“). dictate bräuchte also eine Desktop-Datei mit fester App-ID. | gemessen |
| `CreateSession` | mit App-ID: Antwortcode 0, Sitzung entsteht | gemessen |
| `BindShortcuts` | **Antwortcode 2 (Fehler) nach 1 ms.** Mitschnitt zwischen Portal und Backend: das Backend ruft `org.gnome.Settings.GlobalShortcutsProvider.BindShortcuts` auf, der Bus antwortet `ServiceUnknown`. Den Namen stellt auf GNOME `gnome-control-center`; es ist nicht installiert, und niri besitzt den Namen nicht. | gemessen |
| niri | besitzt von den `org.gnome.*`-Namen auf dem Session-Bus nur `org.gnome.Mutter.*`, `org.gnome.Shell.Introspect` und `org.gnome.Shell.Screenshot`, **nicht** `org.gnome.Shell`; das Binary enthält keine Zeichenkette „GlobalShortcut“ (0 Treffer) | gemessen |
| Drücken und Loslassen | **nicht prüfbar**: ohne erfolgreichen `BindShortcuts` kommt weder `Activated` noch `Deactivated` | gemessen |

**Vorgehen (Wegwerfskript, ändert nichts):** nur der Session-Bus, keine niri-Einstellung, kein Dienst neu gestartet;
eine Wegwerf-Desktop-Datei `at.example.DictateProbe.desktop` lag für die Dauer des Tests in
`~/.local/share/applications/` und ist wieder entfernt; die Sitzung wurde am Ende mit `Session.Close` geschlossen. Skript,
Ausgabe und Mitschnitt: `/home/data/stt-test/portal-probe/` (`gs_probe.py`, `gs_probe3.out`, `gs_monitor4.log`; der
Mitschnitt enthält Bus-Verkehr und gehört nicht ins Repo).

**Was upstream passiert (gelesen, nicht getestet):** niri-wm/niri #3587 (eigene Umsetzung von
`org.gnome.Settings.GlobalShortcutsProvider`, Konfig-Abschnitt für Kürzel) wurde geschlossen und durch **#4744**
„Implement the GlobalShortcuts portal through xdg-desktop-portal-gnome“ ersetzt, **gemergt am 07.11.2026**
(`b4789dd`). Es ist **nicht in v26.04** (25.04.2026, neuester Release; `main` liegt 331 Commits davor). Laut
Quelltext des PR implementiert niri `GrabAccelerator`/`UngrabAccelerator` von `org.gnome.Shell` und sendet
`AcceleratorActivated` **und** `AcceleratorDeactivated` (das Loslassen läuft als eigene Aktion `release`); damit wäre
Halten zum Sprechen grundsätzlich tragbar. Die Kürzel-IDs vergibt die Anwendung, die Tasten bestätigt der Nutzer in
einem Dialog von `xdg-desktop-portal-gnome`. **Ungemessen** bleibt, ob `-gnome` 51.0 mit einem solchen niri ohne
`gnome-control-center` bindet: der Mitschnitt oben zeigt, dass `-gnome` 51.0 den Weg über
`org.gnome.Settings.GlobalShortcutsProvider` geht.

**Folgen und Wiedervorlage**

- Die `input`-Gruppe bleibt Voraussetzung für das Halten (`EVIOCGKEY`). `PLAN-wayland.md` (Tabelle „Portale“ und
  Abschnitt 3.1, Zeile C) ist entsprechend berichtigt: die Aussage „kein GNOME-Portal-Backend“ stimmte nicht, das Backend
  läuft; es scheitert beim Binden.
- **Wiedervorlage K1b:** sobald ein niri-Release mit #4744 erscheint (oder Daniel niri-git nutzt), dasselbe Skript noch
  einmal laufen lassen; zusätzlich ein Tastendruck von Hand mit Zeitstempeln für `Activated` und `Deactivated`.
  Abnahme: `BindShortcuts` Code 0, beide Signale je Druck und Loslassen, `Deactivated` kommt auch, wenn die Taste lange
  gehalten wird und das Fenster wechselt.
- **K2 (nur wenn K1b besteht):** das Portal als **zusätzlicher, optionaler** Weg für Halten und Umschalten, ohne
  `input`-Gruppe; `EVIOCGKEY` bleibt der Weg, wenn das Portal fehlt. Aufwand grob 3 bis 5 Tage (Desktop-Datei mit App-ID,
  `Registry.Register`, Sitzung mit Wiederherstellung nach Neustart von Portal und niri, Tastenwahl über den Dialog,
  Test im verschachtelten niri mit einem Fake-Backend, Härtung der Unit prüfen). Ein Unterschied bleibt: die
  `niri/dictate-binds.kdl`-Tasten sind dann nicht mehr die Quelle; Hinweis in README und Einstellungen.

## 2. Baustein 7: vier kleine Funktionen

Jede Funktion bekommt einen eigenen Branch und PR. Vorgeschlagene Reihenfolge: F1, F4, F3, F2 (die letzte berührt
die Härtung der Unit und braucht die meisten Prüfläufe). Gemeinsame Regeln: sichtbare Texte englisch, README und
`CHANGELOG.md` (Unreleased) mitziehen, neue Module von `dictate-ui` in `MODULES` (`sources_stay_apart`), nie Inhalt
loggen (auch nicht mit `DICTATE_DEBUG`), neue `.rs`-Dateien mit dem Lizenzkopf.

### F1 Letztes Diktat erneut einfügen (Kürzel)

**Befund (gelesen):** `Typed` im Daemon (`src/pipeline/mod.rs`) merkt nur Länge, `app_id` und Zeit des letzten getippten
Diktats für „rückgängig“ (`UNDO_WINDOW`), **nie den Text** (bewusst). `dictate-ui` bekommt den Text mit `Event::Final`
und gibt ihn aus. Der Verlauf (`features.history`) ist aus und kein Ersatz.

**Entwurf:** `dictate-ui` merkt den letzten Endtext **nur im Speicher**, unabhängig von `features.history`, und gibt ihn
auf den neuen Steuerbefehl `dictate-ui paste-last` (`src/ui/ctl.rs`) noch einmal über dieselbe Ausgabekette aus
(Eingabemethode → Einfügen, nicht in Terminals und nicht bei unbekannter `app_id` → eigene Tastatur). Lebensdauer: bis
zum nächsten Endtext, bis zur Sperre oder bis `dictate-ui` endet; ob zusätzlich eine Frist gilt, entscheidet Daniel.

**Abnahme**
1. `dictate-ui paste-last` gibt den letzten Endtext genau einmal aus; zweimal aufgerufen zweimal; nach `Mod+Ctrl+E`
   (Abbruch) bleibt der Text des vorigen Diktats erhalten, ein abgebrochenes Diktat wird nie gemerkt.
2. Stimme der Palette und Textaktionen gelten nicht als Diktat und werden nie gemerkt; das Einfügen sichert und
   stellt die Zwischenablage wie bisher her (`clip.rs`, Regeln zu `x-kde-passwordManagerHint: secret` unverändert).
3. Sperre über logind löscht den Speicher; danach meldet `paste-last` im Overlay „nothing to paste“ (englischer Text).
4. Der Text steht nie im Log (Prüfung mit den Marker-Wörtern wie in `tests/wayland/run.sh`), nie in `history.jsonl`, nicht
   im Daemon (kein neues Feld in `Typed`).
5. Kürzel in `niri/dictate-binds.kdl` (`niri validate -c niri/dictate-binds.kdl`), KDL und Binary nur zusammen
   installieren. **Die Taste wählt Daniel**; vorher gegen espanso (`toggle_key`, `search_shortcut`) und niri-coach
   (`coach-binds*.kdl`) prüfen, weil espanso Tasten über evdev vor niri liest.
6. Tests: Wayland-Ausgabetest im verschachtelten niri (`output.sh`: Fenster, Terminal, unbekannte `app_id`),
   `cargo test --release --bin dictate-ui`.

**Prüfschritte:** `fmt,clippy,unit,wayland`. **Aufwand:** grob 1 Tag.

### F2 Medien pausieren, solange man diktiert (MPRIS)

**Entwurf:** `dictate-ui` fragt über gio den Session-Bus (wie `lock.rs` den logind-Bus) nach `org.mpris.MediaPlayer2.*`.
Beginnt eine Aufnahme (nach einer kurzen Halteschwelle, Vorschlag 300 ms, damit ein kurzes Antippen nichts pausiert),
pausiert es genau die Player mit `PlaybackStatus == Playing` (`Pause()`), merkt sich nur diese und setzt nach dem Ende
(Endtext, Abbruch, Sperre, Fehler, Daemon weg) genau diese mit `Play()` fort, **wenn sie noch `Paused` sind**. Kein
`playerctl` als Kindprozess (Kinder erben die Sandbox der Unit; siehe `docs/systemd-hardening.md`). Metadaten (Titel,
Künstler) werden nie gelesen. Einstellung `features.pause_media`; **Vorgabe aus** (wirkt auf fremde Programme), ob an,
entscheidet Daniel.

**Abnahme**
1. Fake-Player auf einem privaten Session-Bus (wie `tests/wayland/fake-logind`): spielend → beim Halten pausiert,
   beim Loslassen fortgesetzt.
2. Ein Player, der schon pausiert war, bleibt unberührt und wird nie fortgesetzt; zwei Player, einer spielend: nur der
   spielende wird angefasst; hat der Nutzer den Player zwischendurch selbst gestartet, wird er nicht noch einmal
   gestartet.
3. Abbruch, Sperre, Fehler im Daemon, Verschwinden des Players (`NameOwnerChanged`) während der Aufnahme: kein Absturz, kein
   hängender Zustand, nur eigene Pausen werden aufgelöst. Ein Absturz von `dictate-ui` mitten in der Aufnahme lässt die
   Wiedergabe pausiert: akzeptiert und im README genannt.
4. Spielernamen, Titel und Zustände stehen nie im Log.
5. Unter der gehärteten Unit gleich: `DICTATE_UI_UNDER_UNIT=1 tests/wayland/run.sh`, `tests/systemd/probe.py dictate-ui`
   (ob der Session-Bus unter den Netz- und Dateisystem-Optionen erreichbar ist, ist **ungemessen**).
6. Schalter mit Beschreibung auf der passenden Einstellungsseite; neue Klickziele über `layout_log::note`.

**Prüfschritte:** `fmt,clippy,unit,wayland,gui`. **Aufwand:** grob 2 bis 3 Tage.

### F3 Verlauf mit Suche und kurzer Aufbewahrung

**Befund (gelesen):** `features.history` ist aus (Sicherheitsregel in `ROADMAP.md`: Verlauf nur auf Wunsch).
`history.jsonl` (`ts`, `app`, `raw`, `text`, Datei 0600, Ordner 0700) wird nur durch `history.max_entries` (200)
gekürzt; Anfragen `History { limit }` und `HistoryClear`; die Seite „History“ zeigt eine Liste ohne Suche.
Endtexte der Palette stehen nie darin.

**Entwurf:** `history.retention_days` (Vorgabe 7, 0 = nur `max_entries`); gekürzt wird beim Start, beim Anhängen
und beim Lesen (atomar: temporäre Datei mit 0600, dann `rename`). Die Suche läuft **lokal im Daemon**
(`History { query, limit }`, Teilzeichenkette, Groß/klein egal, über `raw` und `text`) und der Suchbegriff wird nie
geloggt. Die Seite zeigt ein Suchfeld und Rohtext gegen Text; einzelne Einträge lassen sich löschen. `features.history`
**bleibt aus**; die Aufbewahrung gilt, wenn Daniel den Verlauf einschaltet. Ob die Vorgabe der Aufbewahrung bei 7 Tagen
liegt, entscheidet Daniel.

**Abnahme**
1. Eintrag älter als `retention_days` ist nach Start, nach dem nächsten Anhängen und bei jedem Lesen weg
   (Uhr im Test eingespeist); `max_entries` gilt zusätzlich.
2. Suche trifft in `raw` und `text`, ignoriert Groß/klein, liefert höchstens `limit` Treffer neueste zuerst; leere Suche =
   bisheriges Verhalten.
3. Datei 0600, Ordner 0700 bleiben nach jedem Umschreiben; ein Absturz beim Umschreiben lässt die alte Datei ganz
   (Test: Abbruch vor `rename`).
4. Weder Suchbegriff noch Treffer erscheinen im Log (Test wie `reports_and_probe_output_never_reach_the_log`).
5. `validate` prüft `retention_days` (Bereich, Vorschlag 0 bis 3650); `dictate-settings` zeigt Feld, Suchfeld und
   Löschen; `HistoryClear` löscht alles wie bisher.
6. Tests: Unit-Tests in `src/pipeline/history.rs`, Protokolltest (`tests/daemon_protocol.rs`), Schritt `gui` mit Lage-Log.

**Prüfschritte:** `fmt,clippy,unit,gui`. **Aufwand:** grob 2 bis 3 Tage.

### F4 `dictate transcribe <Datei>` für Audiodateien

**Befund (gelesen):** `transcribe_batch` (`src/main.rs`) liest über `wav::read_16k_mono` (hound) nur WAV mit 16 kHz Mono.

**Entwurf:** Dekodieren und Umrechnen in reinem Rust, **kein ffmpeg als Kindprozess**: `symphonia` (MPL-2.0, steht in
`deny.toml`) für WAV (beliebige Rate, Kanäle, 16/24/32 Bit, Float), FLAC, MP3, Ogg Vorbis und die Container, die es
beherrscht (M4A/AAC, WebM; **Opus ist ungeprüft** und im Spike zu klären), `rubato` (MIT) auf 16 kHz, Mittelung auf
Mono. Ausgabe wie bisher: Text auf stdout, Zeiten und Ladezeit auf stderr. Bisher geht die ganze Datei in **einem**
Aufruf an die Engine (whisper.cpp arbeitet 30-Sekunden-Fenster ab); ob lange Dateien (zig Minuten) eine Zerlegung über
die VAD-Bereiche (`speech_ranges`) brauchen (Speicher, Wiederholungsschleifen), klärt ein Test mit einer langen
synthetischen Datei, bevor das Verhalten festgelegt wird.

**Abnahme**
1. WAV in anderen Raten, Kanälen und Bittiefen, FLAC, MP3, Ogg Vorbis ergeben dieselben Abtastwerte (Toleranz je
   Codec) und dieselbe Dauer (±1 Frame) wie die Referenz; synthetische Fixtures (Töne, Rauschen, wenige KB, selbst
   erzeugt), keine Aufnahmen von Personen.
2. Nicht unterstützte, beschädigte, abgeschnittene oder sehr große Dateien liefern eine klare Fehlermeldung mit Exit-Code
   ungleich 0, nie Absturz oder unbegrenzten Speicher (Grenzen für Dauer und Größe, eigene Tests mit präparierten
   Dateien).
3. Das Ergebnis der vorhandenen 16-kHz-WAV-Sätze bleibt bit-gleich (`speech`-Schritt unverändert).
4. Neue Abhängigkeiten über `scripts/third-party-licenses.py`, Schritt `licenses` und `cargo deny` grün; Lizenztexte aus
   dem Upstream laut Regel „Drittlizenzen“.
5. README: unterstützte Formate und Grenzen.

**Prüfschritte:** `fmt,licenses,clippy,unit`, dazu `speech` (nur wenn `wav.rs` oder der Pfad der Engine berührt wird).
**Aufwand:** grob 1 bis 2 Tage.

## 3. Baustein 8: freie Modellwahl über mehrere Engines (Schirm), darin der CPU-Weg

**Ziel:** Nutzer wählen das Erkennungsmodell frei, auch ohne NVIDIA-Karte. dictate bekommt eine Engine-Abstraktion;
Whisper (whisper.cpp, wie heute) und ONNX Runtime (zuerst Parakeet v3) sind Engines dahinter, wählbar je Profil in
den Einstellungen; ein Modellkatalog mit Metadaten und „eigene Modelldatei“ je Engine; ein Rückfall auf CPU ohne CUDA;
ein Befehl `dictate bench <Modell>`, der auf den eigenen Testsätzen der Nutzer misst.
**Whisper bleibt die Vorgabe für Deutsch, bis eine Messung etwas anderes zeigt** (Zahlen unten).

### 3.1 Messlage vom 26.11.2026 (Parakeet gegen Whisper, gemessen in Python)

Rechner: Intel i7-7700K (4 Kerne, AVX2, ohne AVX-512/VNNI), RTX 3060, 62 GB RAM; dictate-Standard 4 Threads. Modell:
NVIDIA Parakeet TDT 0.6B v3 (CC-BY-4.0; Namensnennung nötig), ONNX-Export `istupakov/parakeet-tdt-0.6b-v3-onnx`, ausgeführt
mit `onnx-asr` 0.12.0 und `onnxruntime-gpu` 1.32.0 (für die CPU-Läufe mit dem CPU-Ausführungsanbieter). **Alle Zahlen stammen aus Python, nicht aus Rust** (Rohdaten, Skripte,
Logs: `/home/data/stt-test/parakeet-bench/`). 21 Aufnahmen (155 s Audio, davon 18 mit Referenz = 221 Wörter, ein Sprecher,
vorgelesen).

| | Whisper large-v3-turbo q5_0, CUDA (heute) | Whisper q5_0, CPU | Parakeet v3 int8, CPU | Parakeet v3 fp32, CPU | Parakeet v3 fp32, CUDA-EP |
|---|---|---|---|---|---|
| Endlauf je Aufnahme, Mittel / p95 | 429 / 555 ms | 32,0 s / 44,8 s | **509 / 1256 ms** | 591 / 1466 ms | 101 / 138 ms |
| RTF (Σ Latenz / Σ Audio) | 0,058 | 4,34 | 0,069 | 0,080 | 0,014 |
| WER, alle 18 Sätze streng | 6,8 % (15/221) | 7,2 % | 9,0 % (20/221) | 7,7 % | 7,7 % |
| WER ohne t04, t11, t12 (vorher festgelegt: gesprochene Daten, Satzzeichenbefehle) | 3,6 % (7/192) | 4,2 % | 6,8 % (13/192) | 3,6 % | 3,6 % |
| Live: Neuberechnung einer wachsenden Phrase, Median je Länge 1–2 / 2–4 / 4–8 / 8–15 s | 91 / 78 / 116 / 223 ms | 5,1–6,7 s | 172 / 294 / 405 / 746 ms | 237 / 303 / 535 / 898 ms | 101 / 98 / 124 / 125 ms |
| Anteil der Läufe im Takt von 250 ms (dieselben Längen) | 100 / 100 / 94 / 69 % | 0 % | 100 / 38 / 0 / 0 % | 50 / 0 / 0 / 0 % | 100 / 100 / 100 / 100 % |
| Datei auf der Platte | 574 MB | 574 MB | 671 MB | 2551 MB | 2551 MB |
| RAM-Spitze / VRAM-Spitze | 707 MB / 1152 MiB | 1218 MB / 0 | 1561 MB / 0 | 2794 MB / 0 | 1774 MB / 3618 MiB |
| Laden, warm im Seiten-Cache | 0,36 s | 0,16 s | 1,34 s | 1,47 s | 0,91 s |

221 Wörter von einem Sprecher sind zu wenig, um Engines zu reihen: gelesen wird die Richtung, nicht die Nachkommastelle.
(Der Bericht enthält noch eine nachträglich gewählte Sicht, die Whisper begünstigt; sie ist hier nicht benutzt.)

**Folgerungen (abgeleitet):**
- Whisper large-v3-turbo auf der CPU ist für Diktat **nicht brauchbar** (RTF 4,3; jede Live-Vorschau 5 bis 7 s).
- Parakeet **int8 auf der CPU trägt Diktat mit Einschränkungen**: Endläufe von 5 bis 8 s Sprache brauchen 0,3 bis 0,5 s,
  30 s etwa 2,3 s (4 Threads, i7-7700K). Die **Live-Vorschau** hält den Takt von 250 ms nur bis etwa **2 s** neu berechneter
  Audio (172 ms Mittel bei 1–2 s, 294 ms bei 2–4 s); darüber braucht sie ein gedeckeltes Fenster von etwa 2 bis 3 s
  mit festem Text davor (Entwurfsänderung in `stream.rs`, kein Parameter), sonst aktualisiert sich das Overlay mit
  2 bis 4 Hz, ab 8 s mit etwa 1 Hz.
- Auf der GPU ist Parakeet im Endlauf 4-mal schneller (101 gegen 429 ms), in der Live-Vorschau nur bei Phrasen über etwa
  8 s vorn; beide halten dort den Takt. Der Gewinn ist Luft, keine neue Fähigkeit. Whisper bleibt dort Vorgabe.
- int8 kostet Genauigkeit gegenüber fp32 (6,8 % gegen 3,6 % in der vorher festgelegten Sicht); ob das für Nutzer ohne GPU
  tragbar ist, ist die offene Frage (E3).

### 3.2 Grenzen von Parakeet (Messung und Bau)

- **Kein Vokabular-Hinweis:** `vocabulary.terms` (geht bei Whisper als Prompt ein) wirkt nicht; `vocabulary.replacements`
  wirken weiter, sie laufen nach der Erkennung.
- **Kein Sprachhinweis:** die Sprache wird erkannt; eine Ein-Wort-Aufnahme („Ja.“) wurde als „Yeah.“ erkannt (t02).
  Profile mit fester Sprache (`ProfileCfg::language`) wirken dort nicht.
- **Kein Streaming-Modus** in diesem Export: jede Live-Vorschau kodiert das ganze Präfix neu (siehe 3.1).
- **Erster Durchlauf ist langsam** (Planung je neuer Eingabelänge; die erste Runde über alle 21 Aufnahmen brauchte
  10,2 s, bei Whisper-CUDA kostete die erste Aufnahme 0,4 s); Aufwärmen beim Laden einplanen. Auf der GPU kosten neue
  Längen etwa 80 ms zusätzlich; Auffüllen auf feste Längenstufen würde das wohl beseitigen (ungemessen).
- **CUDA-Ausführungsanbieter** braucht **cuDNN 9** zusätzlich zu CUDA (Arch: Paket `cudnn` in `extra`, auf blub
  **nicht installiert**; die Messung nutzte das PyPI-Rad `nvidia-cudnn-cu13`). Die Zusatzerlaubnis in `LICENSE` nennt
  nur Bibliotheken des CUDA Toolkits und den Treiber; **cuDNN hat eine eigene Lizenz** und ist ein offener
  Lizenzpunkt (Teil der Anwaltsfragen). Der Umfang von Baustein 8 ist deshalb der **CPU-Ausführungsanbieter**; die
  GPU-Variante für Parakeet folgt erst nach Klärung (E9).
- **Nicht gemessen:** Sprachwechsel innerhalb eines Satzes, mehrere Sprecher, Spontansprache, leise Mikrofone
  (die Verstärkung `gain.rs` bleibt vorgeschaltet), Stille und Pausen (Silero-VAD bleibt davor), schnellere CPUs mit
  VNNI oder mehr Kernen, Parakeet mit 8 Threads.

### 3.3 Schritte, Abnahme, Aufwand

Reihenfolge ist die Abhängigkeit. Jeder Schritt ein Branch und PR; die volle Suite läuft einmal am Ende.

**E0 Messung in Rust (eigener Schritt mit festem Budget, Ergebnis als Datei, vor jedem Bau).**
Vergleich der Laufzeit-Bibliotheken auf Parakeet v3 int8, CPU, 4 Threads, derselben 21 Aufnahmen:
`ort` (`2.0.0-rc.15` auf crates.io, MIT OR Apache-2.0; die Standard-Features laden beim Bau vorkompilierte
ONNX-Runtime-Bibliotheken herunter, das kollidiert mit dem Bau aus dem Quelltext im AUR und mit dem Schritt `licenses`;
Alternative: gegen das Systempaket `onnxruntime-cpu` aus `extra` binden) gegen `sherpa-onnx` (Rust-Crate 2.0.3,
Apache-2.0; bringt den TDT-Decoder und die Merkmalsberechnung mit, `ort` verlangt dafür eigenen Code).
*Abnahme:* gleiche Transkripte wie `onnx-asr` (Abweichungen je Wort aufgelistet), Latenz in ±25 % der Python-Zahlen,
Erstbauzeit, Binärgröße, Bau ohne Netz, Lizenz- und `licenses`-Befund; Ergebnis als Datei unter `$DICTATE_TEST_ROOT`
und Entscheidung (mit Daniel). *Halt:* nach zwei erfolglosen Ansätzen melden. **Aufwand:** 1 bis 2 Tage.

**E1 Engine-Abstraktion in der Wurzel des Daemons.**
Aus dem vorhandenen `Transcriber` (`transcribe`, `speech_ranges`) und `Engine` (whisper-rs) wird ein Trait mit
`load`, `decode_partial`, `decode_final` und einer Beschreibung der Fähigkeiten: `vocabulary_prompt`, `language_hint`,
Kosten einer Vorschau (gemessen beim Laden, daraus wählt `stream.rs` Takt und Fensterlänge), größte sinnvolle
Vorschau-Länge, benötigte Geräte. Das Silero-VAD läuft heute **über whisper-rs** (`WhisperVadContext`, CPU, 1 Thread); es
wird eine eigene Komponente hinter dem Trait und bleibt zunächst dasselbe (jede Engine bekommt es), damit alle
Engines dieselbe Sprachabgrenzung haben.
*Abnahme:* kein Unterschied im Verhalten der Whisper-Engine (`fmt,clippy,unit,speech,gpu` grün, `speech.tsv` im
Rauschen gleich), `whisper_rs` kommt nur noch in einem Modul vor (`grep`), eine Attrappe der Engine testet Daemon und
Stream ohne Modell, die Fähigkeiten steuern Vokabular-Prompt, Sprachhinweis und Vorschau-Strategie. **Aufwand:** 3 bis 5 Tage.

**E2 Bau ohne CUDA (Cargo-Feature `cuda`, Vorgabe an).**
`whisper-rs` hängt heute fest an `features = ["cuda"]`; `cuda` wird ein Feature von dictate, das durchgereicht wird.
*Abnahme:* `cargo build --release --no-default-features --features ui,gui` (Namen beim Bau festlegen) gelingt in einem
Container ohne CUDA-Toolkit (`archlinux:latest` ohne Paket `cuda`), `dictate transcribe` läuft mit Whisper auf der
CPU (langsam, aber richtig), ein CI-Job übersetzt diese Fassung; PKGBUILD: `cuda` bleibt Bauabhängigkeit, ob es eine
zweite Paketfassung ohne CUDA gibt, entscheidet Daniel. **Aufwand:** 1 bis 2 Tage.

**E3 ONNX-Engine mit Parakeet v3 int8 (CPU-Ausführungsanbieter).**
Neue Engine hinter dem Trait aus E1, Bibliothek nach E0. Fähigkeiten: kein Vokabular-Prompt, kein Sprachhinweis,
Vorschau mit gedeckeltem Fenster (3.1), Aufwärmen beim Laden. Eigene Schwellen in einer eigenen Datei
(`tests/speech/thresholds-parakeet.tsv`, nach derselben Regel abgeleitet; **die Schwellen von Whisper werden nie
gelockert**).
*Abnahme:* Transkripte der 18 Testsätze auf blub ≈ `onnx-asr`-Lauf; Endlauf 20,6 s ≤ 1,5 s (gemessen 1,26 s in Python);
Live-Vorschau mit Obergrenze 250 ms je Durchlauf bei gedeckeltem Fenster; kein Vokabular-Hinweis im Aufruf
(Test), `replacements` wirken; `speech`-Schritt mit der neuen Schwellen-Datei grün; RAM ≤ 2 GB. **Aufwand:** 1 bis 2 Wochen.

**E4 Modellkatalog mit Metadaten.**
`src/models.rs` hat `CATALOG` (`id`, `file`, `url`, `sha256`, `size`, `note`) für Whisper-Dateien; er wird um `engine`,
Sprachen, Platten-, RAM- und VRAM-Bedarf, Lizenz und Namensnennung erweitert. Modelle mit mehreren Dateien (Parakeet:
Kodierer, Decoder/Joiner, Merkmalsnetz, Vokabular) sind ein Eintrag mit Dateiliste, jede mit SHA-256. Dazu **„eigene
Modelldatei“ je Engine**: Pfad oder Ordner, geprüft auf erwartete Dateien und Größe, in den Einstellungen als
„unverified“ gekennzeichnet (keine Prüfsumme, keine Lizenzangabe).
*Abnahme:* Download und SHA-256-Prüfung wie bisher (`gui/downloads.rs`) für Mehrdateien-Modelle, Abbruch bei falscher
Prüfsumme, Anzeige der Daten im Katalog der Einstellungen, Test mit eigenem Modellordner (Attrappe), Lizenzhinweis
(CC-BY-4.0) in der README und im Katalog. **Aufwand:** 2 bis 3 Tage.

**E5 Wahl je Profil und in den Einstellungen.**
`ProfileCfg` bekommt `model` (leer = globales Modell, wie bei `language`); der Daemon lädt beim ersten Gebrauch und hält
höchstens zwei Engines im Speicher (Vorschlag; ein Wechsel kostet das Laden: 0,36 s Whisper-CUDA, 1,3 s Parakeet, bei
kaltem Cache mehr). Die Einstellungen zeigen die Fähigkeiten der gewählten Engine (Vokabular-Hinweis „not used by this
model“, feste Sprache „detected automatically“).
*Abnahme:* Profil mit eigenem Modell schaltet beim Fensterwechsel sauber um, ohne ein Diktat zu verlieren; Speicher- und
VRAM-Grenzen eingehalten (Test mit Attrappe); Einstellungsseite im Schritt `gui` (Lage-Log); Datei speichert nur Abweichungen. **Aufwand:** 2 bis 3 Tage.

**E6 Automatischer Rückfall auf die CPU.**
Findet der Start keine CUDA-Karte (oder scheitert das Laden auf der GPU), wählt der Daemon die CPU-Fassung: das
installierte Parakeet-int8-Modell, sonst eine Meldung im Overlay mit dem Weg zum Herunterladen. **Nie stillschweigend
Whisper-turbo auf der CPU** (RTF 4,3). Die Wahl und der Grund stehen in den Einstellungen und im Log (ohne Inhalte).
*Abnahme:* Test mit versteckter GPU (`CUDA_VISIBLE_DEVICES=`) wählt die CPU-Engine; fehlt auch sie, kommt eine klare
Meldung statt eines Absturzes; die GPU-Fassung bleibt, wenn sie lädt. **Aufwand:** 1 bis 2 Tage.

**E7 `dictate bench <Modell>`.**
Ein Befehl misst ein Modell auf den **eigenen Testsätzen** des Nutzers (Ordner mit `<id>.wav` und `<id>.txt`, wie
`tests/speech/testset.tsv`; sonst nur Latenz): Endlauf je Datei, Live-Vorschau mit wachsender Phrase (250-ms-Takt),
CER und WER (Normalisierung offengelegt), RAM- und VRAM-Spitze, Ladezeit und Aufwärmen; Ausgabe als Tabelle und JSON,
die Wortlaute der Dateien nie im Log. Damit vergleichen Nutzer Modelle auf ihrer Maschine.
*Abnahme:* ein Lauf je Modell ohne zweite Ladung, Zahlen innerhalb 10 % der Handmessung, `--list` zeigt die
Katalogmodelle, ohne Testsätze läuft nur die Latenz; ein Test mit Attrappe-Engine. **Aufwand:** 2 bis 3 Tage.

**E8 Weitere Engines (später, nur bei Bedarf).** Canary und Moonshine, falls sie über ONNX Runtime laufen und die
Fähigkeiten des Traits (E1) genügen; Aufnahme nur mit eigener Messung nach E7 und eigenen Schwellen. **Aufwand:** je nach Modell.

**E9 GPU-Ausführungsanbieter für Parakeet (zurückgestellt).** Erst nach der Klärung der cuDNN-Lizenz (3.2) und mit
`cudnn_conv_algo_search = HEURISTIC` (Standard war langsamer); VRAM-Spitze von 3,6 GiB (ungetunt) gegen 1,15 GiB bei
Whisper beachten. Nutzen gering, solange Whisper auf der GPU gleich schnell im Takt bleibt.

### 3.4 Offene Entscheidungen für Daniel

1. Bibliothek für E3 (nach E0): `ort` gegen `sherpa-onnx`; Umgang mit vorkompilierten Bibliotheken im AUR-Bau.
2. Eine oder zwei Paketfassungen (mit und ohne CUDA) im AUR (E2).
3. Lebensdauer des gemerkten Diktats (F1), Vorgabe von `features.pause_media` (F2), Vorgabe der Aufbewahrung (F3).
4. Taste für „letztes Diktat erneut einfügen“ (F1), geprüft gegen espanso und niri-coach.
5. Ob die Whisper-Vorgabe für Deutsch nach E7 an Nutzern ohne GPU anders gewählt wird (Vorschlag: Rückfall nur für
   Nutzer ohne CUDA, die Vorgabe mit GPU bleibt Whisper).
