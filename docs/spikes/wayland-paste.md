# Spike S3: Einfügen über Data-Control unter niri

Stand 20.11.2026, Block W0. Budget 1,5 h, isolierter Teil gebraucht etwa 5 min (10:40 bis Commit 10:44). Kennzeichnung:
**gemessen** (im verschachtelten niri 26.04), **abgeleitet**, **offen**. Der Teil mit Noctalias Verlauf lief live
mit Daniel (Abschnitt „Live“).

## Aufbau

- Harness wie S4 (`tests/wayland/nested/harness.sh`), GTK-Zielfenster (`wl-gtk-probe target`), virtuelle Tastatur
  aus S2 (`vk kbd`, `chord ctrl+v`).
- Werkzeug `spikes/wayland-paste` (`clip`, `ext_data_control_v1` aus wayland-protocols 0.32.13): `types`, `get`,
  `save DIR`, `offer DIR` (meldet jedes `send` mit Typ und Länge, nie Inhalte), `watch greedy|respect`
  (Stellvertreter eines Verlaufs: `greedy` holt jeden Text, `respect` liest erst `x-kde-passwordManagerHint` und lässt
  `secret` aus).
- Synthetische Auswahl eines „Passwortmanagers“ mit **7 Typen**: `text/plain;charset=utf-8`, `text/plain`,
  `UTF8_STRING`, `text/html`, `image/png` (**5 MB** Zufallsbytes), `application/x-dictate-test` (64 KB),
  `x-kde-passwordManagerHint` = `secret`.
- Skript `spikes/wayland-paste/s3.sh`, drei Läufe (`/home/data/stt-test/logs/wl-spikes/s3/run1..3`); run1/run2 hatten
  einen Fehler im Skript beim Beenden des Anbieters (geerbte FIFO), nur S5 betroffen, in run3 behoben. Alle übrigen
  Befunde in allen drei Läufen gleich.

## Ergebnisse

| Schritt | Ergebnis | Art |
|---|---|---|
| Sichern aller Typen | 7 Typen, 5 308 477 Bytes in 158–180 ms, **byteweise gleich** | gemessen |
| Anbieten des diktierten Texts (5 Texttypen + Hinweis) | der Passwortmanager bekommt `cancelled` (verliert die Auswahl, wie unter X) | gemessen |
| Ctrl+V über die virtuelle Tastatur in GTK | Text kommt an (`Diktat: Grüße aus Wien`), das Ziel holt `text/plain;charset=utf-8` | gemessen |
| Wer holt den diktierten Text ab? | **vor** dem Ctrl+V schon 3 Abholungen: beide Verläufe lesen den Hinweis, der gierige holt den Text; **nach** dem Ctrl+V genau eine (das Ziel) | gemessen |
| Verlauf mit Hinweis | `respect` lässt den diktierten Text aus, `greedy` speichert ihn | gemessen |
| Verlauf ohne Hinweis | beide speichern den diktierten Text | gemessen |
| Zurücklegen | alle **7 Typen samt Hinweis** wieder angeboten, jeder Typ **byteweise gleich** (auch 5 MB, kein Hängen der Pipe: Schreiben im eigenen Thread, Lesen bis EOF) | gemessen |
| Verlauf beim Zurücklegen | sieht eine neue Auswahl; `respect` lässt sie wegen des mitgenommenen Hinweises aus | gemessen |
| **Auswahl nach dem Ende des zurücklegenden Prozesses** | **leer** (`selection empty`): niri hält keinen Inhalt; wer zurücklegt, muss weiter bedienen | gemessen |
| Data-Control sagt nicht, wer abholt | bestätigt: `send` trägt nur den Typ | gemessen |
| **Noctalia beachtet den Hinweis** | live: `wl-copy --sensitive` (Typen geprüft: mit `x-kde-passwordManagerHint`) kam **nicht** in den Verlauf, die Kopie danach ohne Hinweis schon (Daniel hat im Verlauf nachgesehen) | gemessen (live) |
| wl-clipboard 2.3 | `wl-copy --sensitive` bietet den Hinweis an, `wl-paste --watch` setzt dann `CLIPBOARD_STATE=sensitive` | abgeleitet (Manpage) |

## Antworten

- **Sichern, anbieten, Ctrl+V, zurücklegen mit allen Typen samt Hinweis**: geht vollständig und byte-genau.
- **Hinweis gegen Verlauf**: wirkt bei einem Verlauf, der ihn beachtet; **Noctalia beachtet ihn** (live gemessen).
- **Bestätigung des Abholens**: die erste Abholung ist bei einem gierigen Verlauf nicht das Ziel. Verwertbar ist nur
  eine Abholung eines Texttyps **nach** dem gesendeten Ctrl+V; Abholungen vorher stammen von Verläufen.

## Folgen für W2

1. `paste` unter Wayland: sichern (alle Typen, Bytes im Speicher), Text **immer mit `x-kde-passwordManagerHint:
   secret`** anbieten, nach dem Ctrl+V auf die erste Abholung eines Texttyps warten, die **nach** dem Ctrl+V kommt
   (plus kurze Nachfrist, z. B. 300 ms, für Programme, die mehrere Typen holen), sonst Frist wie heute; dann
   zurücklegen mit **allen** gesicherten Typen in ihrer Reihenfolge.
2. **Zurückgelegte Inhalte bedient `dictate-ui` selbst weiter**, bis ein anderer Client die Auswahl übernimmt
   (`cancelled`); endet `dictate-ui`, ist die Zwischenablage leer. Das heißt auch: ein gesichertes Passwort lebt nach
   dem Einfügen im Speicher von `dictate-ui`, und der Passwortmanager besitzt die Auswahl nicht mehr. Ob sein
   Löschen nach Ablauf (KeePassXC, Bitwarden) dann noch greift, hängt davon ab, ob er nach Inhalt oder nach Besitz
   löscht (**offen**). Vorschlag zur Entscheidung in W2: trägt die gesicherte Auswahl den Hinweis `secret`, legt
   dictate sie zurück, bedient sie aber höchstens eine feste Zeit (z. B. 30 s) und gibt sie dann frei; nie loggen,
   nie an den Daemon.
3. Sichern liest jeden Typ (5 MB in 0,16 s); für sehr große Inhalte eine Grenze wie heute unter X (`paste.rs`) und
   bei Überschreitung **nicht einfügen, sondern tippen** statt die Auswahl unvollständig zurückzulegen.
4. Die eigene `selection`-Meldung nach dem Anbieten ignorieren (Data-Control meldet auch die eigene Auswahl).

## Live (mit Daniel, 11:03–11:05 ausgeführt)

Ziel: Beachtet Noctalias Verlauf `x-kde-passwordManagerHint`? Nur **Anzahl und Typen** der Verlaufseinträge ansehen,
nie Inhalte; vorher Einverständnis.

1. Daniel öffnet den Verlauf in Noctalia und merkt sich die Zahl der Einträge (oder: `noctalia msg` mit einer
   Abfrage, die nur Anzahl/Typen liefert, falls vorhanden; vorher in `noctalia msg --help` nachsehen, nichts
   ausführen, was Inhalte ausgibt).
2. Im echten Terminal: `printf 'dictate-s3-test' | wl-copy --sensitive` (Hinweis gesetzt, harmloser Text), dann
   `printf 'dictate-s3-test-2' | wl-copy` (ohne Hinweis).
3. Daniel schaut nach: kam für den ersten Text ein Eintrag dazu? Für den zweiten? Erwartung bei Beachtung: nur der
   zweite.
4. Optional: `wl-copy --sensitive` beenden (`wl-copy` mit `--foreground` und Strg+C) und prüfen, ob Noctalia die
   Auswahl danach wiederherstellt (Typen mit `wl-paste --list-types`).

Ausgeführt: Schritt 2 hat Claude mit Daniels Einverständnis selbst gestartet (`wl-paste --list-types` zeigte beim
ersten Text den Hinweis, beim zweiten nicht), Daniel hat im Verlauf nachgesehen: nur `dictate-s3-test-normal` steht
dort. Schritt 4 nicht ausgeführt (offen: stellt Noctalia eine Auswahl wieder her, deren Besitzer endet?).

## Grenzen

- Verschachteltes niri statt DRM; Data-Control hängt nicht am Backend (abgeleitet).
- Nur ein GTK-Ziel für Ctrl+V; welche Typen Firefox, Chromium, Electron und XWayland-Programme abholen, prüft W2.
- PRIMARY (Textaktionen) nicht Teil dieses Spikes; `ext_data_control` liefert sie über `primary_selection` (abgeleitet,
  Protokoll).
