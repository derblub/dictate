# Spike S5: Springt LockedHint beim Sperren über Noctalia?

Stand 20.11.2026, Block W0. Budget 15 min, gebraucht etwa 10 min (live, 11:05–11:10). Kennzeichnung: **gemessen**,
**abgeleitet**, **offen**.

## Ergebnis live (20.11.)

| Frage | Ergebnis | Art |
|---|---|---|
| Springt `LockedHint` beim Sperren über Noctalia (Mod+Ctrl+L)? | **ja**: 11:06:11.8 `yes` (niri: „locking session“ 11:06:11.58); logind sendet `PropertiesChanged ('org.freedesktop.login1.Session', {'LockedHint': <true>})` an den Sitzungspfad, `gdbus monitor` ohne Root sieht es | gemessen |
| Springt er beim Entsperren zurück? | **ja**, auf `no`, aber nur über `loginctl unlock-session 4` gemessen (siehe unten); Entsperren mit Passwort ging nicht | gemessen |
| Entsperren mit Passwort | **scheiterte** („PAM start failed“). Wahrscheinliche Ursache, unabhängig von dictate: topgrade hat um 10:58 `noctalia` 5.5.0-1 → 5.5.0-2 aktualisiert, während Noctalia seit 19.11. 18:07 lief (`/proc/<pid>/exe` → `/usr/bin/noctalia (deleted)`); Noctalia startet für PAM einen eigenen `pam-helper` aus dem eigenen Binary (Strings `pam-helper`, `/proc/self/exe`). Abhilfe: Noctalia neu starten | gemessen (Fehler, Paketwechsel) / abgeleitet (Ursache) |
| Notausgang | `loginctl unlock-session <Sitzung>` aus einer anderen Shell entsperrt Noctalia sofort (Unlock-Signal von logind; niri: „unlocking session“ 11:07:46) | gemessen |

Folge für W3: `dictate-ui` hört auf `PropertiesChanged` der eigenen Sitzung und löscht beim Wechsel auf `LockedHint =
true` die Sitzungen (`act_clear`/`diag_clear`), dazu beim Start den aktuellen Wert lesen. Kein Rückfall nötig.
Logs: `/home/data/stt-test/logs/wl-spikes/s5/`.

## Befunde ohne Sperren

| Frage | Befund | Art |
|---|---|---|
| Welche logind-Sitzung ist die Wayland-Sitzung? | `loginctl list-sessions`: Sitzung **4** (`Type=wayland`, `Active=yes`, `State=active`), Sitzung 2 ist die alte tty-Sitzung (`closing`); `loginctl show-user blub -p Display` = 4; in Claude-Shells ist `XDG_SESSION_ID=4` | gemessen (einmalig gelesen, 20.11.) |
| `LockedHint` jetzt (entsperrt) | `no` | gemessen |
| Setzt Noctalia `LockedHint`? | Binary enthält `SetLockedHint`, `org.freedesktop.login1.Session`, Pfad `/org/freedesktop/login1/session/auto` | abgeleitet (Strings) |
| Reagiert Noctalia auf logind-Signale? | Strings `Unlock`, „ignoring Unlock signal not sent by logind“, `PrepareForSleep`; ein Signal `Lock` taucht als eigene Zeichenkette nicht auf. Ob `loginctl lock-session` Noctalia sperrt, ist damit **offen** (für dictate nicht nötig, nur für den Hinweis zu `rofi-power` in CLAUDE.md interessant) | abgeleitet |
| Wie findet `dictate-ui` seine Sitzung? | `XDG_SESSION_ID` aus der von `niri-session` importierten Umgebung, sonst `Display` des Users über `org.freedesktop.login1` (System-Bus, ohne Sonderrechte lesbar); nie eine feste Nummer | abgeleitet |

## Live-Ablauf (so ausgeführt)

```bash
S=$(loginctl show-user "$USER" -p Display --value)          # Wayland-Sitzung, heute 4
P=/org/freedesktop/login1/session/_3$S                       # D-Bus-Pfad: führende Ziffer escaped (4 -> _34, 12 -> _312)
L=/home/data/stt-test/logs/wl-spikes/s5; mkdir -p $L
timeout 90 gdbus monitor --system --dest org.freedesktop.login1 --object-path "$P" > $L/monitor.txt &
for i in $(seq 160); do printf '%s %s\n' "$(date +%T.%N)" "$(loginctl show-session "$S" -p LockedHint --value)"; sleep 0.5; done > $L/poll.txt
```

Daniel sperrt mit Mod+Ctrl+L (Noctalia), wartet etwa 10 s, entsperrt. Auswertung: `poll.txt` zeigt `yes` während der
Sperre und `no` danach; `monitor.txt` zeigt `PropertiesChanged` mit `LockedHint` (dann kann `dictate-ui` auf das
Signal hören statt abzufragen). `gdbus monitor` nutzt Match-Regeln, keine Root-Rechte.

## Folgen für W3

- Springt `LockedHint` um: `dictate-ui` beobachtet `PropertiesChanged` der eigenen Sitzung und schickt beim Wechsel
  auf `yes` `act_clear`/`diag_clear` (wie heute `lock.lua`). Zusätzlich beim Start den aktuellen Wert lesen.
- Springt er nicht um: Rückfall über `ext_session_lock`? Das Protokoll erlaubt nur einem Client zu sperren und meldet
  anderen nichts. Zweiter Rückfall: Noctalia-IPC (`noctalia msg`) oder `ext_idle_notifier_v1` (meldet Leerlauf, nicht
  Sperre). Dann Entscheidung Daniel.
