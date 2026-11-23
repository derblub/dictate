# Copyright (C) 2026 Daniel Kurdoghlian
# Pushing Pixels, Vienna <https://pushingpixels.at>
# SPDX-License-Identifier: GPL-3.0-or-later
# Gemeinsame Teile der GUI-Tests (smoke.sh, diagnose.sh, interact.sh, shots.sh): das Einstellungsfenster im
# verschachtelten niri (tests/wayland/nested/harness.sh, kopfloses sway, pixman) gegen den Mock tests/gui/mock_daemon.py.
#
# Nutzung (immer in EINEM bash-Skript; der Harness hält den Injektor über Dateideskriptoren dieses Prozesses):
#   source tests/gui/lib.sh; gui_up || exit 1      niri hoch, Isolation prüfen, Abbau per trap (EXIT/INT/TERM)
#   gui_start [NAME=WERT ...] -- BINARY ARGS...     Mock und Fenster starten (Umgebung für beide), warten
#   gclick X Y; gscroll N; gtype TEXT; gkey CHORD; gshot DATEI
#   gclick_id ID; gui_reveal ID; gui_spot ID         Klickziele aus dem Lage-Log (siehe unten)
#   gui_stop                                        Fenster und Mock beenden, auf das Verschwinden warten
#
# Klickziele (Lesart L46, W6 Block B): das Fenster läuft mit DICTATE_DEBUG und schreibt nach gui.log, wo seine
# benannten Bedienelemente liegen (`dictate-settings: layout <id> x y w h visible|hidden`, `… <id> gone`;
# src/gui/layout_log.rs, IDs dort an den Aufrufen von `layout_log::note`, z. B. `nav:audio`, `apply`,
# `feature_switch`, `diagnose.ping_targets[2].name`). `gclick_id` klickt die Mitte, sobald das Element sichtbar ist und
# ruht; `gui_reveal` scrollt vorher bis dorthin. Ein Ziel, das nicht erscheint, zählt als Fehler. Feste Zahlen nur
# noch für das Scrollen (Bildausschnitte), nicht für Klicks.
#
# Fixtures (Regel 6 in PLAN-wayland.md 6h, nie Daniels Einstellungen): tests/gui/fixtures/gtk-4.0/settings.ini nach
# $XDG_CONFIG_HOME/gtk-4.0/ des Harness (die einzige Quelle der Schrift für GTK 4 unter niri, Messpunkt M1; `Roboto 12`
# wie auf blub, Entscheidung 21; die Familie muss installiert sein, sonst FAIL; `dictate-settings` liest dieselbe
# Datei, Lesart L41), die Palette palette-$GUI_PALETTE.json (dark oder light, Vorgabe dark;
# Tokyo Night aus niri-coach, alle 28 Rollen der Vorlage noctalia/dictate.json.tmpl) als Kopie nach
# $NESTED_DIR/palette.json, exportiert als DICTATE_PALETTE (Mock und Fenster erben sie; ein Test darf die Kopie im
# laufenden Fenster ersetzen, die Fixtures im Repo nie).
#
# Koordinaten sind logische Pixel relativ zur linken oberen Ecke des Fensterinhalts (wie früher auf Xvfb ohne
# Fensterverwalter). Die Ausgabe ist 2600x1950 bei Skalierung 1,625 (1600x1200 logisch, gebrochen wie auf blub): erst
# ab dieser Höhe öffnet niri das schwebende Fenster in seiner vollen Größe 1180x860 (bei 900 logisch waren es 731),
# und `gui_start` prüft genau diese Größe, bevor geklickt wird. Belegung `us`: der Injektor drückt evdev-Codes an
# US-Positionen. Eine Raste des Mausrads verschiebt eine Seite hier um 40 logische Pixel (gemessen; unter Xvfb war
# sie größer, dort nicht gemessen).
#
# Isolation: eigener Laufzeitordner unter $DICTATE_TEST_ROOT/wl-run (harness.sh, scripts/test-root.sh), Socket des Mocks dort; der echte
# Daemon bekommt nie ein set_config (DICTATE_SOCKET entfernt, XDG_RUNTIME_DIR geprüft). Nie die echte Sitzung, nie
# `niri msg` an das echte niri. Abbau per PID/Prozessgruppe, auch bei Abbruch; ein Abbau mit übrig gebliebenen
# Prozessen zählt als Fehler.
#
# Umgebung: LOGS (Ordner für Screenshots und Logs, Vorgabe $DICTATE_TEST_ROOT/logs/gui), GUI_PALETTE (dark|light)

GUI_HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
GUI_ROOT="$(cd "$GUI_HERE/../.." && pwd)"
# shellcheck source=../wayland/nested/harness.sh
source "$GUI_ROOT/tests/wayland/nested/harness.sh"
LOGS="${LOGS:-${DICTATE_TEST_ROOT:-}/logs/gui}"

GUI_PID=""
GUI_MOCK_PID=""
GX=0
GY=0
GTX=0
GTY=0
fail=0

gui_waitfor() { # BEDINGUNG SEKUNDEN: Bedingung (eval) alle 0,2 s, bis sie gilt
    local end=$((SECONDS + $2))
    while ((SECONDS <= end)); do
        eval "$1" && return 0
        sleep 0.2
    done
    return 1
}

gui_cleanup() {
    local rc=$?
    trap - EXIT INT TERM
    gui_stop >/dev/null 2>&1
    if [[ -n ${NESTED_DIR:-} && -d $NESTED_DIR ]]; then
        mkdir -p "$LOGS"
        cp -f "$NESTED_DIR"/*.log "$LOGS"/ 2>/dev/null
    fi
    nested_down || { echo "FAIL  teardown left processes"; rc=1; }
    exit "$rc"
}

gui_up() {
    [[ -x $GUI_ROOT/tests/wayland/nested/target/debug/nested-inject ]] ||
        { echo "FAIL  injector not built (cargo build --locked --manifest-path tests/wayland/nested/Cargo.toml)"; return 1; }
    unset DICTATE_SOCKET DICTATE_UI_SOCKET
    export NESTED_MODE=2600x1950 NESTED_SCALE_CFG=1.625 NESTED_LAYOUT=us NESTED_VARIANT=""
    # Die Fensterregeln des Repos (dictate-settings schwebend) wie auf blub
    export NESTED_EXTRA_KDL="include \"$GUI_ROOT/niri/dictate-binds.kdl\""
    trap gui_cleanup EXIT
    trap 'echo interrupted; exit 130' INT TERM
    nested_up || { echo "FAIL  nested niri did not come up"; return 1; }
    [[ -n $WL_RUN_BASE && $XDG_RUNTIME_DIR == "$WL_RUN_BASE"/* ]] || { echo "FAIL  XDG_RUNTIME_DIR outside $WL_RUN_BASE"; return 1; }
    mkdir -p "$NESTED_RT/dictate" "$LOGS" || return 1
    chmod 700 "$NESTED_RT/dictate"
    gui_fixtures || return 1
    T=$NESTED_DIR
    MOCK_LOG=$NESTED_DIR/mock.log
    GUI_LOG=$NESTED_DIR/gui.log
}

# Schrift und Palette aus tests/gui/fixtures in den Laufzeitordner des Harness (nach nested_up: XDG_* zeigen dorthin)
gui_fixtures() {
    local fx=$GUI_HERE/fixtures want fam got pal=${GUI_PALETTE:-dark}
    [[ $XDG_CONFIG_HOME == "$NESTED_DIR"/* ]] || { echo "FAIL  XDG_CONFIG_HOME outside the harness"; return 1; }
    mkdir -p "$XDG_CONFIG_HOME/gtk-4.0" || return 1
    cp "$fx/gtk-4.0/settings.ini" "$XDG_CONFIG_HOME/gtk-4.0/settings.ini" || return 1
    want=$(sed -n 's/^gtk-font-name=//p' "$fx/gtk-4.0/settings.ini")
    fam=${want% *}
    # fc-match liefert immer etwas (eine fehlende Schrift fällt auf eine andere zurück): nur die Familie fragen (die
    # ganze Pango-Beschreibung „Roboto 12“ läse fontconfig als Familie, Messfehler von M1) und vergleichen
    got=$(fc-match -f '%{family[0]}' "$fam")
    [[ $got == "$fam" ]] || { echo "FAIL  fixture font $fam is not installed (fc-match gives $got)"; return 1; }
    [[ -f $fx/palette-$pal.json ]] || { echo "FAIL  no palette fixture $pal (GUI_PALETTE is dark or light)"; return 1; }
    cp "$fx/palette-$pal.json" "$NESTED_DIR/palette.json" || return 1
    export DICTATE_PALETTE=$NESTED_DIR/palette.json
}

# Fenster von dictate-settings mit der PID des gestarteten Prozesses als JSON (leer, wenn es keins gibt)
gui_window() {
    nmsg -j windows 2>/dev/null | python3 -c 'import json, sys
pid = int(sys.argv[1])
w = [w for w in json.load(sys.stdin) if w.get("app_id") == "dictate-settings" and w.get("pid") == pid]
print(json.dumps(w[0]) if w else "")' "${GUI_PID:-0}"
}
gui_has_window() { [[ -n $(gui_window) ]]; }
gui_any_window() {
    nmsg -j windows 2>/dev/null | python3 -c 'import json, sys
sys.exit(0 if any(w.get("app_id") == "dictate-settings" for w in json.load(sys.stdin)) else 1)'
}

gui_start() { # [NAME=WERT ...] -- BINARY ARGS...
    local envs=()
    while (($#)) && [[ $1 != -- ]]; do envs+=("$1"); shift; done
    shift
    # Ein voriges Fenster muss weg sein, sonst gälte es als das neue
    gui_waitfor '! gui_any_window' 10 || { echo "FAIL  a previous settings window is still open"; fail=$((fail + 1)); return 1; }
    rm -f "$MOCK_LOG" "$NESTED_RT/dictate/control.sock"
    nested_spawn mock-out env "${envs[@]}" python3 "$GUI_HERE/mock_daemon.py" "$NESTED_RT/dictate/control.sock" "$MOCK_LOG"
    GUI_MOCK_PID=$NESTED_LAST_PID
    nested_wait 5 test -S "$NESTED_RT/dictate/control.sock" || { echo "FAIL  mock daemon did not start"; fail=$((fail + 1)); return 1; }
    nested_spawn gui env "${envs[@]}" DICTATE_DEBUG=1 "$@"
    GUI_PID=$NESTED_LAST_PID
    gui_waitfor gui_has_window 15 || { echo "FAIL  window did not appear"; fail=$((fail + 1)); return 1; }
    local geo
    geo=$(gui_window | python3 -c 'import json, sys
w = json.load(sys.stdin)
l = w["layout"]
tx, ty = l["tile_pos_in_workspace_view"]
ox, oy = l["window_offset_in_tile"]
print(tx + ox, ty + oy, *l["window_size"], "floating" if w["is_floating"] else "tiled")')
    read -r GX GY gw gh gmode <<<"$geo"
    if [[ $gw != 1180 || $gh != 860 || $gmode != floating ]]; then
        echo "FAIL  settings window is ${gw}x${gh} $gmode, expected 1180x860 floating (the click targets need it)"
        fail=$((fail + 1))
        return 1
    fi
    # Erstes Bild und Antworten des Mocks abwarten
    sleep 1.5
}

gui_stop() {
    local p
    for p in $GUI_PID $GUI_MOCK_PID; do kill -TERM -- "-$p" 2>/dev/null; done
    for p in $GUI_PID $GUI_MOCK_PID; do
        gui_waitfor "! kill -0 $p 2>/dev/null" 5 || {
            echo "FAIL  still running after TERM: $p"
            fail=$((fail + 1))
            kill -KILL -- "-$p" 2>/dev/null
        }
    done
    if [[ -n $GUI_PID ]] && grep -q panicked "$GUI_LOG" 2>/dev/null; then
        echo "FAIL  panic: $(grep -m1 panicked "$GUI_LOG")"
        fail=$((fail + 1))
    fi
    [[ -n $GUI_PID && -s $GUI_LOG ]] && cat "$GUI_LOG" >>"$NESTED_DIR/gui-all.log"
    GUI_PID=""
    GUI_MOCK_PID=""
}

# Logische Fensterkoordinaten → logische niri-Koordinaten
gui_abs() { python3 -c "print(round($GX + $1, 2), round($GY + $2, 2))"; }
gclick() { # X Y relativ zum Fenster
    # shellcheck disable=SC2046
    nclick $(gui_abs "$1" "$2")
    sleep 0.3
}
gscroll() { # Rasten nach unten (negativ: nach oben), mit der Maus bei 700,500 im Fenster wie früher
    local xy px py
    xy=$(gui_abs 700 500)
    px=$(python3 -c "print(round(${xy% *} * $NESTED_SCALE))") py=$(python3 -c "print(round(${xy#* } * $NESTED_SCALE))")
    nkeys "move:$px,$py" sleep:50 f24 sleep:50 "scroll:$1" sleep:50 f24 sleep:50 f24
    sleep 0.7
}
gtype() { nkeys "text:$1" sleep:50 f24 sleep:50 f24; sleep 0.2; }
gkey() { nkeys "$1" sleep:50 f24 sleep:50 f24; sleep 0.2; }
gshot() { # DATEI: Bild des Fensters (aus dem Bild der ganzen Ausgabe geschnitten)
    nested_shot "$NESTED_DIR/shot.png" || return 1
    python3 - "$NESTED_DIR/shot.png" "$1" "$NESTED_SCALE" "$GX" "$GY" <<'PY'
import sys
from PIL import Image
src, dst, s, x, y = sys.argv[1], sys.argv[2], *map(float, sys.argv[3:])
Image.open(src).crop((round(x * s), round(y * s), round((x + 1180) * s), round((y + 860) * s))).save(dst)
PY
}

# Lage-Log (src/gui/layout_log.rs): letzte Meldung zu ID als „x y w h visible|hidden“ oder „gone“, leer, wenn nie
# gemeldet. Nur die Zeilen des laufenden Fensters (gui.log entsteht je gui_start neu).
gui_spot() {
    awk -v id="$1" '$1 == "dictate-settings:" && $2 == "layout" && $3 == id { l = ($4 == "gone") ? "gone" : $4 " " $5 " " $6 " " $7 " " $8 } END { print l }' "$GUI_LOG" 2>/dev/null
}

gui_target() { # ID [SEKUNDEN]: wartet, bis das Element sichtbar ist und ruht; Mitte in GTX GTY, sonst Fehler
    local id=$1 end=$((SECONDS + ${2:-5})) a="" b x y w h
    while ((SECONDS <= end)); do
        a=$(gui_spot "$id")
        if [[ $a == *" visible" ]]; then
            sleep 0.3
            b=$(gui_spot "$id")
            if [[ $a == "$b" ]]; then
                read -r x y w h _ <<<"$a"
                read -r GTX GTY < <(python3 -c "print(round($x + $w / 2, 1), round($y + $h / 2, 1))")
                return 0
            fi
        else
            sleep 0.2
        fi
    done
    echo "FAIL  click target $id is not visible (last: ${a:-never logged})"
    fail=$((fail + 1))
    return 1
}

gclick_id() { # ID: klickt die Mitte des Elements aus dem Lage-Log
    gui_target "$1" || return 1
    gclick "$GTX" "$GTY"
}

gui_reveal() { # ID [RUNDEN]: scrollt in Schritten von 3 Rasten, bis das Element ganz sichtbar ist
    local id=$1 n=${2:-30} s y
    gui_waitfor "[[ -n \$(gui_spot '$id') ]]" 5 || { echo "FAIL  $id never logged"; fail=$((fail + 1)); return 1; }
    while ((n-- > 0)); do
        s=$(gui_spot "$id")
        [[ $s == *" visible" ]] && return 0
        [[ $s == gone ]] && break
        read -r _ y _ <<<"$s"
        # Unterhalb der Fenstermitte: nach unten scrollen, sonst nach oben
        if python3 -c "import sys; sys.exit(0 if $y > 430 else 1)"; then gscroll 3; else gscroll -3; fi
    done
    echo "FAIL  could not scroll $id into view (last: $(gui_spot "$id"))"
    fail=$((fail + 1))
    return 1
}
