# Spike E10: Wo landet ein Prozess aus niri-IPC `Action::Spawn`?

Stand 21.11.2026, W5 Block A (`PLAN-wayland.md` 6g, Vorlage E10). Budget höchstens zwei Ansätze; gebraucht wurde einer
(Ansatz 1), dazu zwei Wiederholungen desselben Skripts für das Aufräumen. Kennzeichnung: **gemessen**, **abgeleitet**,
**offen**. Skript: `docs/spikes/wayland-spawn.sh` (nur im verschachtelten niri, `tests/wayland/nested/harness.sh`),
Logs: `/home/data/stt-test/logs/w5a/spike3/` (letzter Lauf; `spike/`, `spike2/` mit gleichem Ergebnis).

## Frage

`dictate-ui` startet langlebige Programme der Palette (`launch_app`, `open_project`, `xdg-open`, rofi, `doorcam-watch`)
über niri-IPC statt als eigene Kinder. Landet der Prozess außerhalb des Cgroups des Aufrufers, sodass ein Neustart von
`dictate-ui.service` (`KillMode=control-group`) ihn nicht beendet?

## Ansatz 1: Aufrufer in eigener Unit

Der Aufrufer (`niri msg action spawn -- /usr/bin/sleep …`, dieselbe IPC-Anfrage `Action::Spawn`, die `dictate-ui`
schicken wird) läuft in einer transienten Unit des User-Managers (`systemd-run --user`, `KillMode=control-group` wie
`dictate-ui.service`, eigener Marker in der Umgebung). Das verschachtelte niri liegt im Cgroup der Shell (Orca-Scope),
also in einem anderen als der Aufrufer. Danach wird die Unit gestoppt.

| Frage | Ergebnis | Art |
|---|---|---|
| Cgroup des gestarteten Prozesses | der von **niri** (`app-orca-5597.scope`), nicht der der Aufrufer-Unit (`app.slice/dictate-w5-spike-caller-<pid>.service`) | gemessen |
| Elternprozess | weder Aufrufer noch niri: niri forkt doppelt, der Prozess hängt am nächsten Subreaper (hier `orca-provider-exec`) | gemessen |
| Elternprozess in der echten Sitzung | der User-Manager (nächster Subreaper über `niri.service`) | abgeleitet |
| Überlebt er den Stopp der Aufrufer-Unit (`KillMode=control-group`)? | **ja** | gemessen |
| Umgebung | die von niri, nicht die des Aufrufers (Marker fehlt; `XDG_RUNTIME_DIR` und `WAYLAND_DISPLAY` des verschachtelten niri) | gemessen |
| Arbeitsordner | der von niri; `/usr/bin/env -C /usr/share -- …` setzt ihn wie geplant | gemessen |
| Antwort bei fehlendem Programm (`/nonexistent/…`) | `Handled` (Exit 0, keine Ausgabe), im niri-Log nichts: **kein Startfehler** sichtbar | gemessen |
| Scope `app-niri-*.scope` im verschachtelten niri | keiner, das niri-Log schweigt dazu; der Harness entfernt `DBUS_SESSION_BUS_ADDRESS`, ohne Session-Bus kann niri keinen Scope beim User-Manager anlegen. Eigenschaft des Harness, nicht übertragbar | gemessen (kein Scope) / abgeleitet (Ursache) |

## Echte Sitzung (nur gelesen, `/proc/<pid>/cgroup`, kein IPC)

| Prozess | Cgroup | Art |
|---|---|---|
| niri (`niri --session`) | `session.slice/niri.service` | gemessen |
| `dictate-ui run` | `app.slice/dictate-ui.service` | gemessen |
| alacritty, von niri per Kürzel gestartet | `app.slice/app-niri-alacritty-<pid>.scope` | gemessen |

Dass ein Start über IPC denselben Scope bekommt wie einer über ein Kürzel, ist **abgeleitet** (beide laufen in niri
über dieselbe Spawn-Funktion); gegen die echte Sitzung wird nicht gemessen. **Offen**, beim Live-Test nach Block C/D:
ein Programm aus der Palette starten und `/proc/<pid>/cgroup` lesen (erwartet `app-niri-<name>-<pid>.scope`), danach
`dictate-ui.service` neu starten und prüfen, dass es weiterläuft. Unabhängig vom Scope trägt schon Ansatz 1: der Prozess
gehört niri, nicht dem Aufrufer.

## Arbeitsflächen (Nebenfrage für E2 b)

| Zustand | `Workspaces` des verschachtelten niri | Art |
|---|---|---|
| kein Fenster | `[1 aktiv]` | gemessen |
| je ein Fenster auf 1 und 2 | `[1, 2 aktiv, 3 leer]` | gemessen |

niri führt die leere Arbeitsfläche am Ende in der Liste: „vorhandene plus die leere am Ende“ steckt damit im Kontext,
der Daemon lässt kein `max+1` zu (`resolve/context.rs`, Lesart L11).

## Befund beim Aufräumen

Der erste Lauf öffnete die Fenster mit nacktem `alacritty`; das startete die Login-Shell, die auf blub zellij startet,
und dessen Server löste sich ab (doppelter Fork) und überlebte `nested_down`: `nested_down` sammelt nur direkte Kinder
von niri ein, über IPC gestartete Prozesse sind keine. Abhilfe im Skript: `alacritty -e /usr/bin/sleep …` und am Ende
eine Prüfung aller eigenen Prozesse auf den Laufzeitordner des Harness (`LEFT OVER`). Die Reste wurden per PID beendet.

## Folgen (in `PLAN-wayland.md` 6g eingetragen)

1. **E10 bestätigt:** langlebige Programme über niri `Spawn`; sie überleben einen Neustart von `dictate-ui.service`.
2. **Kein Startfehler von niri:** `find_exe` mit absolutem `argv[0]` vorher ist Pflicht; „Erfolg“ für `pal_chosen` heißt
   „an niri übergeben“ (wie geplant).
3. **Umgebung und Arbeitsordner von niri:** `dictate-ui` kann keine Umgebung mitgeben; `Path=` nur über
   `/usr/bin/env -C <dir> --` (gemessen), Terminal mit festem Befehl `alacritty --working-directory <dir> -e argv…` (E11).
4. **Tests (Block D, `palette.sh`):** über `Spawn` gestartete Programme sind keine Kinder von niri; die Fake-Programme
   müssen sofort enden, und der Test prüft am Ende wie dieses Skript, dass kein Prozess mit dem Laufzeitordner übrig
   ist. Nie die Login-Shell im verschachtelten niri starten.
