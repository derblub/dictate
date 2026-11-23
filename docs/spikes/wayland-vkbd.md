# Spike S2: eigene virtuelle Tastatur (T2) und Eingabemethode (T3) unter niri

Stand 20.11.2026, Block W0. Budget 2 h, gebraucht etwa 20 min (10:23 bis Commit 10:40, Nachmessung mit Positivkontrolle 10:52). Kennzeichnung: **gemessen** (im
verschachtelten niri 26.04, winit-Backend, Mod = Alt), **abgeleitet**, **offen**.

## Aufbau

- Harness `tests/wayland/nested/harness.sh` (siehe `wayland-gtk.md`), niri mit Daniels Belegung `de nodeadkeys`,
  `repeat-delay 275`, `repeat-rate 35`, xwayland-satellite des verschachtelten niri an (eigenes X-Display `:4`,
  geprüft ≠ `:0`). Kürzel im Test: `Mod+Q`, `Mod+Shift+Q`, `Mod+W` legen je eine Markierungsdatei an.
- Werkzeug `spikes/wayland-vkbd` (`vk`, wayland-client 0.31.15, wayland-protocols-misc 0.3.12):
  - `vk kbd`: `zwp_virtual_keyboard_v1` mit **erzeugter Belegung wie wtype** (eine Taste je Zeichen, ein Keysym je
    Taste, Shift/Control/Alt/Super auf den ersten vier Tasten mit `modifier_map`), Befehle `type`, `prepare` (ganze
    Belegung vorab hochladen), `chord`, `down`/`up`, `destroy`, `exit` (Prozessende ohne Loslassen).
  - `vk im`: `zwp_input_method_v2`, meldet `activate`/`deactivate`/`done`/`content_type`, `commit TEXT`.
- Ziele: GTK-4-Eingabefeld (`wl-gtk-probe target`, Wire-Log), **Alacritty** und **xterm** (XWayland) mit einem
  Python-Leser im Raw-Modus, der die empfangenen Bytes in eine Datei schreibt; **Firefox 160** und **Chromium 157**
  (ohne und mit `--enable-wayland-ime --wayland-text-input-version=3`) mit Wegwerfprofil im privaten Laufzeitordner
  und einer Seite, die den Inhalt des Textfelds in den Fenstertitel schreibt.
- Physische Tasten (gehaltener Modifikator) über das virtuelle Gerät auf dem kopflosen sway (niri sieht sie wie eine
  echte Tastatur).
- Skripte `s2.sh` (GTK, Alacritty, xterm), `s2c.sh` (Modifikatoren in Terminals, Zeilenumbruch, zweite
  Eingabemethode), `s2b.sh` (Browser, Diagnose Chromium). Logs: `/home/data/stt-test/logs/wl-spikes/s2/run2` und `run3` (mit Positivkontrolle der Kürzel und klassischen Latin-1-Keysyms, gleiche Ergebnisse),
  `…/s2c/run1`, `…/s2b/run4`, `…/s2b/diag1`, `…/s2b/diag2`.

Testtext: `Grüße, Maß: äöü ÄÖÜ ß € – „ok“` (Umlaute, ß, €, Halbgeviertstrich, deutsche Anführungszeichen).

## Ergebnisse T2 (eigene virtuelle Tastatur)

| Frage | Ergebnis | Art |
|---|---|---|
| Umlaute, ß, €, – „“ in GTK 4 | exakt | gemessen |
| … in Alacritty (nativ) | exakt (48 Bytes UTF-8) | gemessen |
| … in xterm (XWayland über xwayland-satellite) | exakt | gemessen |
| … in Firefox | exakt (Belegung beim Tippen wachsend und vorab hochgeladen) | gemessen |
| … in Chromium 157 | **`ß:` von `Maß:` fehlt**, in beiden Flag-Varianten, auch mit vorab hochgeladener Belegung und mit klassischen Latin-1-Keysyms statt `U+`-Keysyms | gemessen |
| Ursache Chromium | Chromium deutet **evdev-Code 14 (`KEY_BACKSPACE`) als Rücktaste**, wenn das Keysym kein Buchstabe ist: liegt `:`, `,` oder `-` auf Code 14, wird das vorige Zeichen gelöscht und das Zeichen selbst fehlt (`bc:|` → `b|`); `q` und `ß` auf Code 14 kommen richtig an. In kleinen Belegungen (Code 14 frei) war jeder Text exakt | gemessen |
| Belegung nach dem Tippen | physische Tasten haben danach wieder Daniels Belegung (Code 21 ergibt `z` wie in `de`) in GTK, Alacritty, xterm; niri schickt bei jedem Wechsel zwischen virtueller und physischer Tastatur die jeweilige Belegung neu (23 `wl_keyboard.keymap` für einen Satz) | gemessen |
| **Kürzel durch die virtuelle Tastatur** | Positivkontrolle: dieselben Akkorde über die physische Tastatur lösen `Mod+Q` und `Mod+Shift+Q` aus (Markierungsdateien da). `chord alt+shift+q` und `chord alt+q` nur über das virtuelle Gerät lösen **keine** niri-Kürzel aus | gemessen |
| **Physisch gehaltener Modifikator + virtuelle Taste** | Alt bzw. Ctrl physisch gehalten, virtuelles `w`/`c`: **kein Kürzel**, und das Programm sieht die Taste **ohne** Modifikator: GTK `modifiers(0)` vor der virtuellen Taste, Alacritty und xterm bekommen ein schlichtes `c` (kein `^C`, kein `ESC c`) | gemessen |
| Abbruch: Taste gedrückt, dann `destroy` | **niri lässt nicht los**: kein `key(…, 0)` an den Client, das Programm wiederholt die Taste selbst weiter (GTK 17 × `b` in 2 s, Alacritty/xterm > 60 Zeichen), bis irgendein anderes Tastenereignis kommt | gemessen |
| Abbruch: Taste gedrückt, Prozessende ohne Loslassen (wie SIGKILL) | genauso: Dauerwiederholung im Programm, in GTK, Alacritty und xterm | gemessen |
| Abbruch mit Loslassen (`down`/`up`) | genau ein Zeichen | gemessen |
| Welchen Modifikator-Zustand sieht das virtuelle Gerät? | keinen: `zwp_virtual_keyboard_v1` hat keine Ereignisse; ohne fokussierte eigene Fläche gibt es auch kein `wl_keyboard` | abgeleitet (Protokoll) |

## Ergebnisse T3 (Eingabemethode)

| Frage | Ergebnis | Art |
|---|---|---|
| GTK 4 | `activate` sofort nach dem Anlegen (auch wenn das Feld schon fokussiert war), Commit exakt | gemessen |
| Alacritty | `activate`, `content_type … Terminal`, Commit exakt; `\n` im Commit kommt als `\r` an (= Return, führt in einer Shell die Zeile aus) | gemessen |
| xterm (XWayland) | **kein `activate`**, Commit ohne Wirkung | gemessen |
| Firefox 160 | `activate`, Commit exakt | gemessen |
| Chromium 157 **ohne** IME-Flags | `activate` (Inhaltstyp mit `AutoCapitalization`), Commit exakt | gemessen |
| Chromium 157 mit `--enable-wayland-ime --wayland-text-input-version=3` | wie ohne Flags | gemessen |
| Slack (Electron 45, Chrome 156) | wahrscheinlich wie Chromium 157 | abgeleitet (Version), **live offen** |
| Orca (Electron 43, Chrome 150), Start nur mit `--disable-features=Vulkan` | ob text-input-v3 ohne Flag an ist, unbekannt | **offen, live** |
| Physisches Tippen bei gebundener Eingabemethode | unverändert (keine Tastatur-Übernahme ohne `grab_keyboard`) | gemessen |
| Commit bei physisch gehaltenem Alt | kein Kürzel, Text ohne Modifikator | gemessen |
| Zweite Eingabemethode am selben Sitz | bekommt **nie** ein Ereignis, auch kein `unavailable`, und **auch nicht, nachdem die erste zerstört wurde** (neu fokussiert); eine danach neu angelegte arbeitet | gemessen |
| Zeit bis `activate` | 0,3–0,5 s nach Prozessstart im verschachtelten Aufbau (Software-Rendering, nicht aussagekräftig) | gemessen, live offen |

## Antworten auf die offenen Fragen des Plans

- **Umlaute und ß**: T2 und T3 liefern exakten Text in GTK, Alacritty, Firefox; T2 auch in XWayland; T2 in Chromium
  nur mit einer Belegung, die Sondercodes meidet (siehe Folgen).
- **Gehaltenes Super**: unter niri ungefährlich für Kürzel (die virtuelle Tastatur läuft an niris Kürzeln vorbei) und
  für Programme (eigener Modifikator-Zustand des Geräts). Das gilt im verschachtelten niri mit Mod = Alt; dass der
  Weg auf DRM derselbe ist (smithay-Code für virtuelle Tastaturen hängt nicht am Backend), ist abgeleitet. Regel 2 aus
  Abschnitt 4 wird damit vom Sicherheits- zum Komfortpunkt; W2 behält das Warten trotzdem (siehe Folgen), weil der
  Live-Test auf DRM aussteht und XWayland-Programme über xwayland-satellite gingen (dort ebenfalls ohne Modifikator
  gemessen).
- **Abbruch mitten im Anschlag**: niri lässt beim Zerstören **nicht** los, auch nicht beim Prozessende. Regel 3 ist
  damit Pflicht: T2 drückt und lässt jede Taste in derselben Nachricht los (kein Halten über einen `flush` hinaus) und
  sendet vor `destroy` für jede gedrückte Taste das Loslassen; ein Absturz zwischen Drücken und Loslassen erzeugt
  Dauerwiederholung im Zielprogramm bis zum nächsten Tastendruck.
- **Loslassen beim Zerstören**: nein (siehe oben).

## Folgen für W2

1. **T2-Belegung**: Tasten für Zeichen nur auf evdev-Codes ohne Sonderbedeutung legen. Gemessen ist nur Code 14
   (Backspace); dass Chromium auch andere Codes nach ihrer physischen Bedeutung behandelt (1 Esc, 15 Tab, 28 Enter,
   96 KP-Enter, 102–111 Navigation, 113–128 Media/Power, 183–194 F13–F24), ist abgeleitet. Am sichersten nur die
   Buchstaben- und Ziffernblock-Codes (2–11, 16–25, 30–38, 44–50 usw.) und bei mehr Zeichen eine neue Belegung für
   den nächsten Abschnitt. Return, Tab, Backspace nur als echte Tasten auf ihren echten Codes.
   Test in W2: Chromium mit einem Text, der jede Sonderposition belegen würde.
2. **Belegung einmal je Auftrag hochladen** (`prepare`), dann tippen; das Wachsen beim Tippen ging auch, kostet aber je
   neues Zeichen eine volle Belegung an den Client.
3. **Abbruch**: Drücken und Loslassen jeder Taste in einem Zug, Loslassen aller gehaltenen Tasten vor `destroy`, beim
   Beenden des Dienstes (`SIGTERM`) ebenso; Unit-Test, dass kein Pfad eine Taste gedrückt lässt.
4. **Modifikatoren**: Das Warten auf losgelassene Modifikatoren (EVIOCGKEY, S1) bleibt für `Ctrl+V` beim Einfügen und
   als Vorsicht für den DRM-Live-Test; für reinen Text ist es nach dieser Messung nicht nötig. Live auf DRM einmal
   prüfen: Super gehalten, virtuelles Tippen, kein Kürzel.
5. **T3**: Eingabemethode nur für die Dauer einer Ausgabe (oder einer Aufnahme) halten, nie dauerhaft: eine zweite
   Eingabemethode (fcitx5, ibus, später) bliebe sonst bis zu ihrem Neustart tot, und umgekehrt bliebe die von dictate
   tot, wenn schon eine andere läuft. Entscheidung je Ausgabe: kommt für das fokussierte Feld innerhalb einer Frist ein
   `activate`, Commit; sonst T2. `\n` im Commit wirkt in Terminals als Return (wie heute getipptes Return).

## Entscheidung für Daniel: Eingabemethode (T3) ja/nein je Programm

| Programm | T2 | T3 | Empfehlung |
|---|---|---|---|
| GTK-4-Programme (auch dictate-Panels) | geht | geht | T3 |
| Alacritty | geht | geht | T3 (Terminal: kein Einfügen, `\n` = Return wie bisher) |
| Firefox | geht | geht | T3 |
| Chromium | geht nur mit Sondercode-freier Belegung | geht ohne Flags | T3 |
| Slack (Electron 45) | wie Chromium | wahrscheinlich | T3, im Live-Test bestätigen; Rückfall T2 automatisch |
| Orca (Electron 43) | wie Chromium | unbekannt | automatisch (T3 wenn `activate`, sonst T2); live prüfen |
| XWayland-Programme (Spotify u. a.) | geht | geht nicht | T2 |

**Empfehlung:** T3 als Vorgabe **automatisch**: dictate legt die Eingabemethode mit Beginn der Aufnahme an, nutzt sie,
wenn das Zielfeld sie aktiviert, und tippt sonst mit T2; nach der Ausgabe wird sie zerstört. Dazu eine Config-Liste
`typing.keyboard_only_app_ids` für Programme, die T3 annehmen, aber falsch umsetzen (heute keins bekannt).
Begründung: T3 liefert exakten Text ohne Belegungstausch und ohne Sondercode-Falle, kann bei einem Absturz keine Taste
hängen lassen und löst nie Kürzel aus; T2 bleibt für XWayland und `Ctrl+V` nötig und ist mit den Folgen 1–3 sicher.
Preis: eine Fallunterscheidung mehr in W2 und die Eingabemethode belegt während der Ausgabe den einzigen Platz am Sitz.
Alternative „nur T2“ wäre einfacher, verlangt aber die Belegungsregeln aus Folge 1 und hat das Absturzrisiko.
**Entschieden (Daniel, 20.11.): T3 automatisch.**

## Grenzen

- Verschachteltes niri (winit) statt DRM; Kürzel- und Modifikatorbefunde gelten für smithays Pfad der virtuellen
  Tastatur, der nicht vom Backend abhängt (abgeleitet). Live-Prüfung auf DRM in W2.
- Slack und Orca nicht gestartet (echte Profile, Einzelinstanz-Sperre könnte das echte Fenster aktivieren); Chromium
  157 steht für Electron. Live-Test in W2.
- Qt-Programme nicht geprüft (keins installiert, das Daniel nutzt; abgeleitet: Qt 6 spricht text-input-v3).
