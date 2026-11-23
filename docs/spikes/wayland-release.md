# Spike S1: Loslassen von Mod+E über den Tastenzustand (EVIOCGKEY)

Stand 20.11.2026, Block W0. Budget 1 h, gebraucht etwa 15 min (Live-Termin mit Daniel, 10:55–11:03). Kennzeichnung:
**gemessen**, **abgeleitet**, **offen**. Variante C wurde im verschachtelten niri gemessen (siehe S4).

## Ergebnis live (20.11., echter Rechner, nur lesen)

| Frage | Ergebnis | Art |
|---|---|---|
| Geräte mit E und Super | `event5` und `event9` (Ducky Keyboard, zwei Schnittstellen), `event4` (Logitech G903, Maus mit Tastenschnittstelle), `event25` (**Espanso virtual device**, uinput von Espanso) | gemessen |
| Erkennt EVIOCGKEY Drücken und Loslassen von E und Super? | **ja**: 22 Drücke von E über 120 s, alle mit Loslassen gesehen, beide Reihenfolgen (erst E, erst Super los), linke und rechte Super-Taste, kürzester Druck 81 ms, längster 946 ms. Alle Wechsel kamen über `event5`; `event9`, G903 und Espanso blieben still | gemessen |
| Abfrageabstand (Ziel 20 ms) | Median 20,1 ms, p99 24,6 ms, Maximum 35,2 ms (5 871 Abfragen, unter `strace`) | gemessen |
| Kosten je `ioctl` | Median 39 µs, Maximum 18,6 ms, beides **unter `strace`** (aufgebläht); ohne `strace` nicht gemessen | gemessen |
| Latenz des Loslassens | höchstens Abfrageabstand: im Mittel etwa 10 ms, schlimmstenfalls etwa 35 ms | abgeleitet aus den Zahlen |
| Nie `read()` auf ein Eingabegerät | `strace -e openat,read,ioctl`: die einzigen `read` gehen auf `libgcc_s.so.1`, `libc.so.6` und `/proc/self/maps` (Laden), dann nur `ioctl` (alle `= 96`) | gemessen |
| Zweite Tastatur | keine angeschlossen; Mod auf einem, E auf einem anderen Gerät nicht geprüft (abgeleitet abgedeckt) | offen |
| Mod+E ohne Belegung | niri reicht die Taste ans fokussierte Fenster weiter (Alacritty bekam „e“); mit `dictate-binds.kdl` verbraucht niri sie | gemessen / abgeleitet |

Folge für W1: **3.2 A bleibt** (Loslassen sicher, Latenz unter 35 ms). Alle Geräte mit E abfragen, auch weitere
Schnittstellen derselben Tastatur. Das Espanso-Gerät gehört dazu: Espanso tippt über uinput, sein E zählte als
gedrückt, wenn Espanso während einer Aufnahme ein E tippt (selten, hinnehmen; abgeleitet).

## Vorbereitet

Werkzeug `spikes/wayland-release` (`keystate`, ohne Abhängigkeiten):

- öffnet `/dev/input/event*` nur lesend und nicht blockierend, nutzt **nur** `ioctl`: `EVIOCGBIT(EV_KEY)` (kann das
  Gerät E und Super?), `EVIOCGNAME`, `EVIOCGKEY` (Bitfeld der gerade gedrückten Tasten). **Nie `read()`** auf ein
  Eingabegerät; der Ereignisstrom bleibt ungelesen.
- `keystate list`: Tastaturen mit `KEY_E` und `KEY_LEFTMETA`.
- `keystate watch --interval-ms 20 --seconds 60`: Wechsel von E, linker und rechter Super-Taste je Gerät mit Zeit;
  am Ende Zahl der Abfragen, Kosten je `ioctl` (Median/Maximum in µs), tatsächlicher Abstand der Abfragen
  (Median/p99/Maximum in ms).
- Gebaut und `cargo clippy` sauber; auf dem echten Rechner nur im Live-Termin gestartet. Logs: `/home/data/stt-test/logs/wl-spikes/s1/` (`watch2.txt`, `strace2.txt`; der erste Lauf `watch.txt` lief, bevor Daniel die Anleitung sah, und zeigt keine Wechsel).

## Abgeleitet (ohne Messung)

| Frage | Befund | Art |
|---|---|---|
| Sieht EVIOCGKEY das Loslassen? | Der Kernel aktualisiert das Tastenbitfeld des Geräts beim Verarbeiten jedes Ereignisses (`input_handle_event` → `dev->key`), unabhängig davon, ob jemand liest; EVIOCGKEY kopiert dieses Feld. Loslassen ist also spätestens beim nächsten Abfragen sichtbar | abgeleitet (Kernel-Semantik) |
| Latenz | Abfrageabstand (20 ms) plus Dauer eines `ioctl` (µs-Bereich): im Mittel 10 ms, höchstens etwa 20 ms plus Planungsverzug | abgeleitet, Zahlen live |
| Mehrere Tastaturen | jede Tastatur hat ihr eigenes Bitfeld; `keystate` fragt alle mit E und Super ab, gedrückt = auf irgendeinem Gerät gedrückt. Mod auf Tastatur A und E auf Tastatur B ist damit abgedeckt | abgeleitet |
| Geräte ohne `/dev/input` (VNC über `wayvnc`, virtuelle Tastaturen anderer Programme, eigene virtuelle Tastatur von dictate) | erscheinen nicht, ihr Loslassen ist unsichtbar → Höchstdauer der Aufnahme als Rückfall (wie im Plan, Vorschlag 60 s, Config) | abgeleitet |
| Rechte | Gruppe `input` liest jedes Gerät; EVIOCGKEY zeigt nur den Zustand, nicht die Folge der Tasten. Die Code-Regel „nur EVIOCGKEY“ (plus Test, z. B. dass das Modul kein `read` auf diese Dateien enthält, und die Abfrage nur während einer Aufnahme läuft) bleibt nötig | abgeleitet |
| Wann ist „losgelassen“? | Heute (Awesome) beendet das Loslassen der Kürzel-Taste. Vorschlag für W1: Ende, sobald E auf keinem Gerät mehr gedrückt ist (Super egal), mit 80 ms Entprellung wie heute (`debounce` in `init.lua`) | abgeleitet, Entscheidung W1 |

## Variante C (aus 3.2, im verschachtelten niri gemessen)

Overlay greift die Tastatur beim Kürzel: das Loslassen von **E kommt nicht an** (niri verschluckt Drücken und
Loslassen der Kürzel-Taste), das Loslassen von Super schon; das Zielfenster verliert den Fokus. Taugt nicht als
Ersatz für A. Einzelheiten in `wayland-gtk.md`, Abschnitt „Variante C“. **gemessen**

## Live-Ablauf (so ausgeführt)

Vorher: Daniel ist einverstanden, dass ein Programm 60 s lang den **Zustand** von E und Super liest (kein
Tastenprotokoll). Mod+E ist in niri heute frei; das fokussierte Programm bekommt dabei Super+E (in einem leeren
Alacritty harmlos).

```bash
K=spikes/wayland-release/target/debug/keystate
L=/home/data/stt-test/logs/wl-spikes/s1
mkdir -p $L
$K list | tee $L/list.txt
strace -f -e trace=read,ioctl -o $L/strace.txt $K watch --interval-ms 20 --seconds 60 | tee $L/watch.txt
grep -c 'read(' $L/strace.txt          # nur Lesen von Bibliotheken/proc, keines auf /dev/input-Deskriptoren
```

Während der 60 s: (1) Mod+E 2 s halten, erst E, dann Super loslassen; (2) Mod+E halten, erst Super, dann E
loslassen; (3) 5 × kurz tippen (unter 150 ms); (4) falls eine zweite Tastatur angeschlossen ist: Super auf der einen,
E auf der anderen. Daniel sagt dabei grob, wann er loslässt.

Auswertung: jeder Fall erscheint als E=1 → E=0 im richtigen Gerät; Abstand p99 und Maximum; Kosten je ioctl;
`strace` zeigt keine `read` auf die geöffneten `event`-Deskriptoren (Nachweis für die Code-Regel).

## Folgen für W1

1. `dictate-ui` (nicht der Daemon) fragt den Zustand ab, nur zwischen „Aufnahme läuft (Halten)“ und Ende; sonst sind
   die Geräte geschlossen.
2. Höchstdauer als Rückfall (VNC, kein lesbares Gerät), Umschalten (B) als Config-Rückfall.
3. Geräteliste bei jeder Aufnahme neu (Hotplug, z. B. Tastatur am Dock).
4. Nach dem Live-Termin hier Zahlen eintragen; ändert er nichts am Ergebnis (Loslassen sicher erkannt, Latenz unter
   etwa 30 ms), bleibt 3.2 A wie entschieden.
