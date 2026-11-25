#!/usr/bin/env bash
# Copyright (C) 2026 Daniel Kurdoghlian
# Pushing Pixels, Vienna <https://pushingpixels.at>
# SPDX-License-Identifier: GPL-3.0-or-later
# Prüfschritt `wayland` (scripts/check.sh): dictate-ui im verschachtelten niri (kopfloses sway, pixman) gegen einen
# Mock-Daemon. Prüft das Overlay (Layer-Fläche ohne Fokus, klick-durch nach Map, Größenänderung und erneutem Map,
# sichtbar gezeichnet), die Kürzel aus niri/dictate-binds.kdl (die Datei des Repos, per include), Halten und
# Loslassen mit Entprellung, Rückfall Umschalten (E nie gedrückt gesehen, kein Gerät lesbar), Abbrechen, Umschalten,
# Abbruch beim Beenden, `dictate-ui cancel` ohne Oberfläche, eine zweite Instanz und dass keine Mitschrift ins Log
# geht. Danach die Textausgabe (W2, tests/wayland/output.sh): Eingabemethode, virtuelle Tastatur, Einfügen über die
# Zwischenablage samt Zurücklegen, Passwort-Hinweis, Modifikatoren, Abbruch, Terminals (Alacritty, xterm über XWayland).
# Danach die Textaktionen (W3, tests/wayland/actions.sh): PRIMARY, Panel, Copy, Replace, Schließen, Stop, das
# Diagnose-Panel samt Boot-Hinweis (W4, tests/wayland/diagnose.sh), die Befehlspalette (W5, tests/wayland/palette.sh:
# Kandidaten, Ausführen gegen die Fake-Programme, confirm, KI, Stimme ins Panel, Übergaben an die anderen Panels) und
# die Sperre aller Panels
# (tests/wayland/lock.sh) gegen ein falsches logind auf dem privaten System-Bus aus harness.sh, zuletzt das Abmelden aus
# der Palette (tests/wayland/logout.sh, beendet das verschachtelte niri).
#
# Isolation: tests/wayland/nested/harness.sh (eigener Laufzeitordner unter $DICTATE_TEST_ROOT/wl-run, nie die echte
# Sitzung, nie `niri msg` an das echte niri). Der Tastenzustand kommt aus einer Datei (`--fake-keys`), nie aus
# /dev/input: die Tasten des Injektors erscheinen dort nicht, uinput ist in Tests verboten. Abbau aller Prozesse per
# PID/Prozessgruppe, auch bei Abbruch (trap).
#
# Aufruf: tests/wayland/run.sh (nach `cargo build --release --bin dictate-ui` und dem Bau des Injektors)
#   DICTATE_UI_UNDER_UNIT=1 dictate-ui unter der Härtung von systemd/dictate-ui.service (docs/systemd-hardening.md)
#   DICTATE_TEST_LOGS=DIR   Logs dorthin kopieren (Vorgabe $DICTATE_TEST_ROOT/logs/wl-test, Testordner aus
#                           scripts/test-root.sh)
set -uo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
UI="$ROOT/target/release/dictate-ui"
[[ -x $UI ]] || { echo "FAILED: $UI is not built"; exit 1; }
[[ -x $HERE/nested/target/debug/nested-inject && -x $HERE/nested/target/debug/nested-clip ]] || { echo "FAILED: injector/clip not built (tests/wayland/nested)"; exit 1; }

# shellcheck source=nested/harness.sh
source "$HERE/nested/harness.sh"
LOGS="${DICTATE_TEST_LOGS:-${DICTATE_TEST_ROOT:-}/logs/wl-test}"
[[ -n $WL_RUN_BASE ]] || { echo "FAILED: no usable test folder (DICTATE_TEST_ROOT, message above)"; exit 1; }

pass=0
fail=0
ok() { echo "ok   $*"; pass=$((pass + 1)); }
bad() { echo "FAIL $*"; fail=$((fail + 1)); }
check() { # NAME CMD...: ok, wenn CMD gelingt
    local name=$1
    shift
    if "$@"; then ok "$name"; else bad "$name"; fi
}

MOCK_PID=""
UI_PID=""
cleanup() {
    local rc=$?
    trap - EXIT INT TERM
    [[ -n $UI_PID ]] && kill -TERM "$UI_PID" 2>/dev/null
    [[ -n $MOCK_PID ]] && kill -TERM "$MOCK_PID" 2>/dev/null
    [[ -n $MOCK_PID ]] && wait "$MOCK_PID" 2>/dev/null
    if [[ -n ${NESTED_DIR:-} && -d $NESTED_DIR ]]; then
        mkdir -p "$LOGS"
        cp -f "$NESTED_DIR"/*.log "$NESTED_DIR"/*.png "$NESTED_DIR"/*.bin "$LOGS"/ 2>/dev/null
    fi
    nested_down || { echo "FAIL teardown left processes"; rc=1; }
    [[ -n ${MARK:-} && -d $MARK ]] && rm -rf "$MARK"
    exit "$rc"
}
trap cleanup EXIT
trap 'echo interrupted; exit 130' INT TERM

# niri startet die Programme der Kürzel mit seiner Umgebung: dictate-ui aus dem Build zuerst im PATH
export PATH="$ROOT/target/release:$PATH"
unset DICTATE_SOCKET DICTATE_UI_SOCKET DICTATE_PALETTE
export NESTED_MODE=2600x1462 # 1600x900 logisch bei 1,625
# xwayland-satellite für xterm (W2); Mod+Q legt eine Marke an (Positivkontrolle: löst eine virtuelle Taste kein Kürzel aus?)
export NESTED_XWAYLAND=1
mkdir -p "$WL_RUN_BASE"
MARK="$(mktemp -d "$WL_RUN_BASE/m.XXXXXX")"
export NESTED_EXTRA_KDL="include \"$ROOT/niri/dictate-binds.kdl\"
binds {
    Mod+Q { spawn \"touch\" \"$MARK/mod-q\"; }
}
window-rule {
    match app-id=\"^dictate-test-target\$\"
    open-maximized true
}"
nested_up || { echo "FAILED: nested niri did not come up"; exit 1; }
mkdir -p "$LOGS"
# Alles Folgende spricht nur mit dem eigenen Laufzeitordner (Mock statt Daemon); sonst sofort aufhören
[[ -n $WL_RUN_BASE && $XDG_RUNTIME_DIR == "$WL_RUN_BASE"/* ]] || { echo "FAILED: XDG_RUNTIME_DIR outside $WL_RUN_BASE"; exit 1; }
# System-Bus nur der private aus harness.sh (dictate-ui hört dort auf logind): nie das echte logind
[[ ${DBUS_SYSTEM_BUS_ADDRESS:-} == "unix:path=$NESTED_RT/"* ]] || { echo "FAILED: DBUS_SYSTEM_BUS_ADDRESS is not the private test bus"; exit 1; }
D=$NESTED_DIR
MOCK_LOG=$D/mock.log
KEYS=$D/keys
UI_LOG=$D/ui.log
TARGET_LOG=$D/target.log
: >"$MOCK_LOG"
# Ausgabe in W1-Abschnitten: tippen (das Klick-Fenster hat kein Textfeld; Einfügen wartete dort bis zur Frist)
echo '{"typing": {"output": "type"}}' >"$D/config.json"
mkdir -p "$NESTED_RT/dictate"
chmod 700 "$NESTED_RT/dictate"
# Standardpfade im eigenen Laufzeitordner: $XDG_RUNTIME_DIR/dictate/control.sock (Daemon) und …/ui.sock
python3 "$HERE/mock_daemon.py" "$NESTED_RT/dictate/control.sock" "$MOCK_LOG" 120 >"$D/mock-out.log" 2>&1 &
MOCK_PID=$!
nested_wait 5 test -S "$NESTED_RT/dictate/control.sock" || { echo "FAILED: mock daemon"; exit 1; }
# Falsches logind auf dem privaten System-Bus (tests/wayland/fake-logind belegt den Namen auf „seinem“ Session-Bus):
# eigene Sitzung _32 (Nummer 2, "auto"), fremde _33; GetSession-Aufrufe in logind.calls
LOGIND_CALLS=$D/logind.calls
nested_spawn logind env DBUS_SESSION_BUS_ADDRESS="$DBUS_SYSTEM_BUS_ADDRESS" FAKE_LOGIND_LOG="$LOGIND_CALLS" python3 "$HERE/fake-logind"
nested_wait 5 grep -q ready "$D/logind.log" || { echo "FAILED: fake logind"; exit 1; }

count() { grep -c " $1\$" "$MOCK_LOG"; }
at_least() { (($(count "$1") >= $2)); }
last_ns() { grep " $1\$" "$MOCK_LOG" | tail -n 1 | cut -d' ' -f1; }
now_ns() { date +%s%N; }
press() { nkeys "$1" sleep:50 f24 sleep:50 f24; }
layers() { nmsg -j layers 2>/dev/null; }
overlay_up() { layers | python3 -c 'import json,sys; sys.exit(0 if any(l.get("namespace")=="dictate-overlay" for l in json.load(sys.stdin)) else 1)'; }
overlay_down() { ! overlay_up; }
overlay_no_keyboard() {
    layers | python3 -c 'import json,sys
ls=[l for l in json.load(sys.stdin) if l.get("namespace")=="dictate-overlay"]
sys.exit(0 if len(ls)==1 and ls[0].get("keyboard_interactivity")=="None" and ls[0].get("layer")=="Overlay" else 1)'
}
focused_app() { nmsg -j focused-window | python3 -c 'import json,sys; w=json.load(sys.stdin); print((w or {}).get("app_id") or "")'; }
target_focused() { [[ $(focused_app) == dictate-test-target ]]; }
clicks() { grep -c '^click ' "$TARGET_LOG"; }
more_clicks() { (($(clicks) > $1)); }
# Klick mitten ins Overlay (unten mittig, Rand 48, Höhe mindestens 70): muss beim Zielfenster ankommen
OX=800
OY=820
click_through() {
    local before
    before=$(clicks)
    nclick "$OX" "$OY" || return 1
    nested_wait 3 more_clicks "$before"
}
# Bild des Ausgangs: an der Klickstelle die dunkle Fläche des Overlays, darin der Akzent der Palette (Welle), darüber
# das helle Fenster. Akzent: `primary` der Palette (ohne Datei die eingebaute, Tokyo Night #7aa2f7)
ACCENT=7aa2f7
overlay_drawn() {
    nested_shot "$D/$1.png" || return 1
    python3 - "$D/$1.png" "$NESTED_SCALE" "$OX" "$OY" "$ACCENT" <<'PY'
import sys
from PIL import Image
img = Image.open(sys.argv[1]).convert("RGB")
s, ox, oy = float(sys.argv[2]), int(sys.argv[3]), int(sys.argv[4])
ar, ag, ab = (int(sys.argv[5][i:i + 2], 16) for i in (0, 2, 4))
lum = lambda p: 0.3 * p[0] + 0.59 * p[1] + 0.11 * p[2]
inside = img.getpixel((round(ox * s), round(oy * s)))
outside = img.getpixel((round(ox * s), round(300 * s)))
teal = 0
for y in range(round((oy - 60) * s), round((oy + 35) * s)):
    for x in range(round((ox - 400) * s), round((ox + 400) * s)):
        r, g, b = img.getpixel((x, y))
        if abs(r - ar) < 30 and abs(g - ag) < 30 and abs(b - ab) < 30:
            teal += 1
print(f"inside={inside} outside={outside} accent_px={teal}")
sys.exit(0 if lum(inside) < 110 and lum(outside) > 150 and teal > 20 else 1)
PY
}

# Programme der Palette (L22): dictate-ui sucht sie nur hier (`--fake-programs`), nie im PATH. Jedes Fake schreibt nur
# seinen Namen und sein argv als Zeile nach $FAKE_LOG (Pfad fest eingetragen: über niri `Spawn` gestartete Fakes haben die
# Umgebung von niri). Ohne diesen Ordner läuft kein Test (ein Return auf `reboot` erreichte sonst das echte systemctl).
FAKES=$D/fakes
FAKE_LOG=$D/fake-programs.log
make_fake() { # NAME [SHELL-ZEILEN NACH DEM LOG]
    cat >"$FAKES/$1" <<FAKE
#!/bin/sh
# Fake-Programm der Befehlspalette (tests/wayland/run.sh, L22): schreibt nur Namen und argv, eine Zeile je Aufruf
{ printf '%s' '$1'; for a in "\$@"; do printf ' [%s]' "\$a"; done; echo; } >>'$FAKE_LOG'
${2:-}
FAKE
    chmod 755 "$FAKES/$1"
}
mkdir -p "$FAKES"
: >"$FAKE_LOG"
for f in loginctl noctalia playerctl xdg-open rofi copyq bemoji kitty claude sh dictate-settings dictate-fake-app; do
    make_fake "$f"
done
# das automatisch gewählte Terminal (alacritty vor kitty, `ui::pal_exec::TERMINALS`) schreibt dazu seinen Arbeitsordner:
# „Projekt öffnen“ startet es über `env -C <Projektordner>`
make_fake alacritty "echo \"alacritty-cwd [\$(pwd)]\" >>'$FAKE_LOG'"
# pactl liest die Lautstärke aus einer Datei (die Palette hebt höchstens bis 100 %); systemctl suspend scheitert wie auf blub
# (sleep.conf), die Meldung erscheint im Overlay
make_fake pactl "[ \"\$1\" = get-sink-volume ] && cat '$D/pactl-volume'
exit 0"
make_fake systemctl "if [ \"\$1\" = suspend ]; then echo 'Call to Suspend failed: Sleep verb \"suspend\" is disabled by configuration' >&2; exit 1; fi"
[[ $FAKES == "$WL_RUN_BASE"/* && -x $FAKES/systemctl && -x $FAKES/dictate-fake-app ]] || { echo "FAILED: no fake programs in $FAKES"; exit 1; }

nested_spawn target python3 "$HERE/target.py"
nested_wait 10 target_focused || { echo "FAILED: target window did not get the focus"; exit 1; }

echo 0 >"$KEYS"
# DICTATE_UI_UNDER_UNIT=1: dictate-ui läuft unter der Härtung von systemd/dictate-ui.service (flüchtige Unit im
# User-Manager mit genau dieser Umgebung, tests/systemd/under-unit.py). Schreiben darf es dann nur, was ihm die Unit
# lässt: seinen Laufzeit-, Zustands- und Cache-Ordner und /tmp (hier TMPDIR); dazu das Log der Fake-Programme, die als
# Kurzläufer in derselben Unit laufen. Alles andere im Testordner ist für dictate-ui nur lesbar
UI_UNDER=()
if [[ -n ${DICTATE_UI_UNDER_UNIT:-} ]]; then
    mkdir -p "$XDG_STATE_HOME/dictate" "$XDG_CACHE_HOME" "$TMPDIR"
    UI_UNDER=(python3 "$ROOT/tests/systemd/under-unit.py" dictate-ui --rw "$NESTED_RT/dictate" --rw "$XDG_STATE_HOME/dictate" --rw "$XDG_CACHE_HOME" --rw "$TMPDIR" --rw "$FAKE_LOG" --)
    echo "dictate-ui runs under the hardening of systemd/dictate-ui.service"
fi
start_ui() {
    # NIRI_SOCKET wie live (der User-Manager hat ihn): nur der verschachtelte Socket im eigenen Laufzeitordner
    nested_spawn ui env DICTATE_LOG=dictate_ui=debug,info NIRI_SOCKET="$NESTED_NIRI_SOCKET" XDG_SESSION_ID="${UI_SESSION_ID:-2}" DBUS_SYSTEM_BUS_ADDRESS="${UI_SYSBUS:-$DBUS_SYSTEM_BUS_ADDRESS}" "${UI_UNDER[@]}" "$UI" run --fake-keys "$KEYS" --fake-programs "$FAKES" ${UI_BOOT_MS:+--boot-hint-ms "$UI_BOOT_MS"}
    UI_PID=$NESTED_LAST_PID
    nested_wait 10 test -S "$NESTED_RT/dictate/ui.sock" || return 1
    # nur mit den Fakes weiter (die Zeile kommt vor dem Steuer-Socket)
    grep -q "palette programs only from $FAKES (test mode)" "$D/ui.log" || { echo "FAILED: dictate-ui without fake programs"; return 1; }
}
start_ui || { echo "FAILED: dictate-ui did not start"; cat "$UI_LOG"; exit 1; }
UI_LOG=$D/ui.log
nested_wait 5 grep -q 'connected to the daemon' "$UI_LOG" || bad "ui connects to the daemon"
check "warm-up: first frame drawn at start (invisible)" nested_wait 10 grep -q 'renderer ready' "$UI_LOG"
check "warm-up: overlay hidden again before the first shortcut" nested_wait 5 overlay_down

# --- 1. Halten und Loslassen (Mod+E, verschachtelt Alt+E) -----------------------------------------------------
echo 1 >"$KEYS"
press alt+e
check "hold: Mod+E starts listening" nested_wait 3 at_least start 1
check "overlay is mapped as layer surface" nested_wait 5 overlay_up
check "overlay: layer Overlay, keyboard None" overlay_no_keyboard
check "overlay: target window keeps the focus" target_focused
# Erst klicken, wenn niri das Overlay zeigt (sonst prüfte der Klick nichts; Gegenprobe ohne leere Eingaberegion)
check "overlay: drawn after map" nested_wait 5 overlay_drawn map
check "overlay: click passes through after map" click_through
sleep 0.6
check "hold: no stop while E is down" test "$(count stop)" -eq 0
check "overlay: click passes through after resize (transcript)" click_through
check "overlay: drawn with transcript" overlay_drawn hold
check "overlay: target window still focused" target_focused
check "overlay: target never lost the focus" bash -c "! grep -q 'focus out' '$TARGET_LOG'"
t_rel=$(now_ns)
echo 0 >"$KEYS"
if nested_wait 3 at_least stop 1; then
    dt=$((($(last_ns stop) - t_rel) / 1000000))
    if ((dt >= 80 && dt <= 600)); then ok "hold: release stops after debounce ($dt ms)"; else bad "hold: release stops after debounce ($dt ms, want 80..600)"; fi
else
    bad "hold: release stops listening"
fi
check "overlay hides after the session" nested_wait 5 overlay_down

# --- 2. Palette wechselt (Noctalia schreibt palette.json neu), zweite Aufnahme: umgefärbt, erneut gemappt -------
mkdir -p "$XDG_STATE_HOME/dictate"
sed -e 's/#7aa2f7/#e0af68/' "$HERE/palette-tokyo.json" >"$XDG_STATE_HOME/dictate/palette.json.tmp"
mv "$XDG_STATE_HOME/dictate/palette.json.tmp" "$XDG_STATE_HOME/dictate/palette.json"
check "palette: reloaded after the file changed" nested_wait 3 grep -q 'palette from' "$UI_LOG"
ACCENT=e0af68
echo 1 >"$KEYS"
press alt+e
check "hold again: start" nested_wait 3 at_least start 2
check "overlay remapped" nested_wait 5 overlay_up
check "overlay: drawn after remap in the new palette" nested_wait 5 overlay_drawn remap
check "overlay: click passes through after remap" click_through
echo 0 >"$KEYS"
check "hold again: stop" nested_wait 3 at_least stop 2

# --- 3. Abbrechen (Mod+Ctrl+E) während des Haltens: danach stoppt kein Loslassen ------------------------------
nested_wait 5 overlay_down
echo 1 >"$KEYS"
press alt+e
check "cancel: start" nested_wait 3 at_least start 3
press ctrl+alt+e
check "cancel: Mod+Ctrl+E cancels" nested_wait 3 at_least cancel 1
echo 0 >"$KEYS"
sleep 0.4
check "cancel: release after cancel sends no stop" test "$(count stop)" -eq 2

# --- 4. E nie gedrückt gesehen (Kürzel über VNC): Umschalten statt Halten -------------------------------------
nested_wait 5 overlay_down
echo 0 >"$KEYS"
press alt+e
check "latch (E never seen): start" nested_wait 3 at_least start 4
sleep 0.6
check "latch (E never seen): keeps listening" test "$(count stop)" -eq 2
check "latch: hint in the log" grep -q 'key release not detected' "$UI_LOG"
press alt+e
check "latch: next Mod+E stops" nested_wait 3 at_least stop 3

# --- 5. Kein Gerät lesbar: ebenso Umschalten ------------------------------------------------------------------
nested_wait 5 overlay_down
rm -f "$KEYS"
press alt+e
check "no device: start" nested_wait 3 at_least start 5
sleep 0.5
check "no device: keeps listening" test "$(count stop)" -eq 3
press alt+e
check "no device: next Mod+E stops" nested_wait 3 at_least stop 4

# --- 6. Freihändig (Mod+Shift+E) ------------------------------------------------------------------------------
nested_wait 5 overlay_down
press shift+alt+e
check "toggle: Mod+Shift+E" nested_wait 3 at_least toggle 1
nested_wait 3 overlay_up
press shift+alt+e
check "toggle: second Mod+Shift+E" nested_wait 3 at_least toggle 2
check "toggle carries the app_id of the focused window" grep -q ' toggle_app dictate-test-target$' "$MOCK_LOG"

# --- 7. Beenden während einer Aufnahme bricht sie ab -----------------------------------------------------------
nested_wait 5 overlay_down
echo 1 >"$KEYS"
press alt+e
check "exit: start" nested_wait 3 at_least start 6
kill -TERM "$UI_PID"
check "exit: SIGTERM during a recording cancels it" nested_wait 3 at_least cancel 2
check "exit: dictate-ui ends" nested_wait 5 bash -c "! kill -0 $UI_PID 2>/dev/null"
check "exit: control socket removed" test ! -e "$NESTED_RT/dictate/ui.sock"
UI_PID=""

# --- 8. Ohne Oberfläche: cancel geht an den Daemon, hold meldet den Fehler -------------------------------------
"$UI" cancel >"$D/cancel-direct.log" 2>&1
check "no ui: dictate-ui cancel exits 0" test $? -eq 0
check "no ui: cancel reached the daemon" nested_wait 3 at_least cancel 3
"$UI" hold >"$D/hold-direct.log" 2>&1
check "no ui: dictate-ui hold exits 2" test $? -eq 2

# --- 9. Zweite Instanz wird abgelehnt, ohne die erste zu stören ------------------------------------------------
cp "$UI_LOG" "$D/ui-first.log"
start_ui || bad "restart dictate-ui"
WAYLAND_DISPLAY="$NESTED_NIRI_WL" timeout 10 "$UI" run --fake-keys "$KEYS" --fake-programs "$FAKES" >"$D/ui-second.log" 2>&1
check "second instance exits 1" test $? -eq 1
check "second instance names the reason" grep -q 'another dictate-ui is running' "$D/ui-second.log"
check "first instance keeps its socket" test -S "$NESTED_RT/dictate/ui.sock"

# --- 11.–20. Textausgabe (W2) ---------------------------------------------------------------------------------
# shellcheck source=output.sh
source "$HERE/output.sh"

# --- 21.–26. Textaktionen (W3) ---------------------------------------------------------------------------------
# shellcheck source=actions.sh
source "$HERE/actions.sh"

# --- D1.–D12. Diagnose-Panel und Boot-Hinweis (W4) ---------------------------------------------------------------
# shellcheck source=diagnose.sh
source "$HERE/diagnose.sh"

# --- P1.–P27. Befehlspalette (W5) ------------------------------------------------------------------------------------
# shellcheck source=palette.sh
source "$HERE/palette.sh"

# --- 27.–33. Sperre (W3c, Diagnose W4, Palette W5) ----------------------------------------------------------------
# shellcheck source=lock.sh
source "$HERE/lock.sh"

# --- P28. Abmelden (W5): beendet das verschachtelte niri, darum zuletzt ---------------------------------------------------
# shellcheck source=logout.sh
source "$HERE/logout.sh"

# --- 10. Nie Mitschrift, Text oder Inhalt der Zwischenablage ins Log -----------------------------------------------
check "log: no transcript, final text or clipboard content" bash -c "! grep -h -e 'Testsatz' -e 'Umlauten' -e 'Grüße' -e 'Zweite Zeile' -e 'Eingefügt' -e 'Passwort123' -e 'alte Zwischenablage' -e 'echo eins' -e 'abcdefghij' -e 'danach' -e 'Während des Sicherns' -e 'Neben dem Passwort' -e 'Über X' -e 'Markierter Text' -e 'Alter Text' -e 'Ersetzter Text' -e 'Erklärung' -e 'Antwortliste' -e 'ls -l /tmp' '$D'/ui*.log"
# Diagnose (W4): Bericht, Pseudonyme im Klartext, Platzhalter samt Wert, Symptom, pstore-Abbild
check "log: no report, plain values, symptom or pstore dump" bash -c "! grep -h -e 'diagmarker' -e 'geheimhost' -e 'erika.musterfrau' -e 'musterfrau' -e 'bootet' -e 'nicht check' -e 'Kernel panic' -e 'nvidia_isr' -e 'REDACTED' -e 'host-1' -e 'user-1 ' '$D'/ui*.log"
# Befehlspalette (W5): Eingaben, Anzeige der Kandidaten und Fenstertitel tragen `palmarker` (Absichten wie `reboot` stehen
# zu Recht im Log), dazu die getippten Wörter als Eingabe
check "log: no palette input, candidate or window title" bash -c "! grep -h -e 'palmarker' -e 'fakeapp' -e 'from-palette' -e 'Fenster der Palette' '$D'/ui*.log"
# Übergaben (Block D): Frage, CLIPBOARD und Passwort tragen `palmarker` bzw. eigene Marker; Eingabewörter der Übergaben
check "log: no question, clipboard text or password of the palette" bash -c "! grep -h -e 'Monad' -e 'Inhalt der Zwischenablage' -e 'Passwort-der-Palette' -e 'diagunit' -e 'polish' -e 'backup' '$D'/ui*.log"
# Übrige Absichten (Block D2): die Eingabewörter (Adresse, Suchbegriff und Projektordner tragen `palmarker`, oben)
# Fehlermeldung des Fakes systemctl suspend (steht nur im Overlay) und Ausgabe des Fakes pactl
check "log: no error text or output of the fake programs" bash -c "! grep -h -e 'Sleep verb' -e 'front-left' '$D'/ui*.log"
check "log: no input word of the other intents" bash -c "! grep -h -E 'qq(focus|view|prev|move|tabbed|normal|float|full|max|close|proj|folder|url|search|lock|volset|volup|mute|media|door|cliphist|emoji|settings|shotfull|shotwin|shotarea|logout|poweroff|suspend)' '$D'/ui*.log"
grep -h -E 'GTK ready|first frame|shown in' "$D/ui-first.log" | sed 's/^/info /' | head -n 5

echo "---"
echo "wayland: $pass ok, $fail failed (logs: $LOGS)"
((fail == 0))
