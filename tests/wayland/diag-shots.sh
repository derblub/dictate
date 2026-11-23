#!/usr/bin/env bash
# Copyright (C) 2026 Daniel Kurdoghlian
# Pushing Pixels, Vienna <https://pushingpixels.at>
# SPDX-License-Identifier: GPL-3.0-or-later
# Rauchtest des Diagnose-Panels (W4 „Rest Block B“ Punkt 5): dictate-ui im verschachtelten niri gegen den Mock-Daemon
# (`diag_*` in mock_daemon.py), Ablauf Profile → Einträge → Fortschritt → Vorschau (Feld „What is wrong?“ mit Tab und
# Klick, Enter und Esc) → Senden → Antwort (Werte, Beleg) → Prüfrunde → Schließen, Screenshots je Ansicht, Dauer des
# Neuaufbaus mit vollem Budget (48 KiB). Am Ende: kein Testtext im Log von dictate-ui. Manuell, kein Prüfschritt (die
# Abnahme `tests/wayland/diagnose.sh` baut Block C darauf auf).
#
# Aufruf: tests/wayland/diag-shots.sh [AUSGABEORDNER]  (nach `cargo build --release --bin dictate-ui` und dem Injektor;
#         Vorgabe $DICTATE_TEST_ROOT/logs/diag-shots, Testordner aus scripts/test-root.sh)
# Isolation wie run.sh: tests/wayland/nested/harness.sh, eigener Laufzeitordner, nie die echte Sitzung.
set -uo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
UI="$ROOT/target/release/dictate-ui"
[[ -x $UI ]] || { echo "FAILED: $UI is not built"; exit 1; }
[[ -x $HERE/nested/target/debug/nested-inject ]] || { echo "FAILED: injector not built (tests/wayland/nested)"; exit 1; }

# shellcheck source=nested/harness.sh
source "$HERE/nested/harness.sh"
OUT="${1:-${DICTATE_TEST_ROOT:-}/logs/diag-shots}"
[[ -n $WL_RUN_BASE ]] || { echo "FAILED: no usable test folder (DICTATE_TEST_ROOT, message above)"; exit 1; }

pass=0
fail=0
ok() { echo "ok   $*"; pass=$((pass + 1)); }
bad() { echo "FAIL $*"; fail=$((fail + 1)); }
check() {
    local name=$1
    shift
    if "$@"; then ok "$name"; else bad "$name"; fi
}

MOCK_PID=""
cleanup() {
    local rc=$?
    trap - EXIT INT TERM
    [[ -n $MOCK_PID ]] && kill -TERM "$MOCK_PID" 2>/dev/null && wait "$MOCK_PID" 2>/dev/null
    if [[ -n ${NESTED_DIR:-} && -d $NESTED_DIR ]]; then
        mkdir -p "$OUT"
        cp -f "$NESTED_DIR"/*.log "$NESTED_DIR"/*.png "$OUT"/ 2>/dev/null
    fi
    nested_down || { echo "FAIL teardown left processes"; rc=1; }
    exit "$rc"
}
trap cleanup EXIT
trap 'echo interrupted; exit 130' INT TERM

export PATH="$ROOT/target/release:$PATH"
unset DICTATE_SOCKET DICTATE_UI_SOCKET DICTATE_PALETTE
export NESTED_MODE=2600x1950 NESTED_LAYOUT=us NESTED_VARIANT=""
# DIAG_EXTRA_KDL: weitere Regeln für Versuche (z. B. Breite je Titel); DIAG_ONLY_OPEN=1: nur öffnen und Größe melden
export NESTED_EXTRA_KDL="include \"$ROOT/niri/dictate-binds.kdl\"
${DIAG_EXTRA_KDL:-}"
nested_up || { echo "FAILED: nested niri did not come up"; exit 1; }
[[ -n $WL_RUN_BASE && $XDG_RUNTIME_DIR == "$WL_RUN_BASE"/* ]] || { echo "FAILED: XDG_RUNTIME_DIR outside $WL_RUN_BASE"; exit 1; }
[[ ${DBUS_SYSTEM_BUS_ADDRESS:-} == "unix:path=$NESTED_RT/"* ]] || { echo "FAILED: not the private test bus"; exit 1; }
D=$NESTED_DIR
MOCK_LOG=$D/mock.log
UI_LOG=$D/ui.log
: >"$MOCK_LOG"
mkdir -p "$NESTED_RT/dictate" && chmod 700 "$NESTED_RT/dictate"
python3 "$HERE/mock_daemon.py" "$NESTED_RT/dictate/control.sock" "$MOCK_LOG" 120 >"$D/mock-out.log" 2>&1 &
MOCK_PID=$!
nested_wait 5 test -S "$NESTED_RT/dictate/control.sock" || { echo "FAILED: mock daemon"; exit 1; }
nested_spawn logind env DBUS_SESSION_BUS_ADDRESS="$DBUS_SYSTEM_BUS_ADDRESS" FAKE_LOGIND_LOG="$D/logind.calls" python3 "$HERE/fake-logind"
nested_wait 5 grep -q ready "$D/logind.log" || { echo "FAILED: fake logind"; exit 1; }
# Programme der Palette nur aus einem leeren Ordner (L22): hier wird nichts gestartet, nie der echte Suchpfad
mkdir -p "$D/fakes"
nested_spawn ui env DICTATE_LOG=dictate_ui=debug,info NIRI_SOCKET="$NESTED_NIRI_SOCKET" XDG_SESSION_ID=2 "$UI" run --fake-keys "$D/keys" --fake-programs "$D/fakes"
nested_wait 10 test -S "$NESTED_RT/dictate/ui.sock" || { echo "FAILED: dictate-ui"; exit 1; }
nested_wait 5 grep -q 'connected to the daemon' "$UI_LOG"

panel_json() { nmsg -j windows | python3 -c 'import json,sys
w=[w for w in json.load(sys.stdin) if w.get("app_id")=="dictate-panel"]
print(json.dumps(w[0]) if w else "")'; }
panel_up() { [[ -n $(panel_json) ]]; }
panel_down() { ! panel_up; }
panel_size() { panel_json | python3 -c 'import json,sys; print(*json.load(sys.stdin)["layout"]["window_size"])'; }
panel_focused() { [[ $(nmsg -j focused-window | python3 -c 'import json,sys; print((json.load(sys.stdin) or {}).get("app_id"))') == dictate-panel ]]; }
renders() { grep -c "diagnose: rendered $1 in" "$UI_LOG"; }
more_renders() { (($(renders "$1") > $2)); }
acts() { grep -c " $1" "$MOCK_LOG"; }
more_acts() { (($(acts "$1") > $2)); }
key() { nkeys "$1" && sleep 0.05; }
settle() { sleep 0.5; }
shot() { sleep 0.4; nested_shot "$D/diag-$1.png" && echo "info shot diag-$1.png ($(panel_size))"; }
item_xy() {
    local l
    l=$(grep "panel: layout $1 " "$UI_LOG" | tail -n 1)
    [[ -n $l ]] || return 1
    panel_json | python3 -c 'import json,sys
w = json.loads(sys.stdin.read())["layout"]
x, y, bw, bh = (float(v) for v in sys.argv[1].split()[-4:])
tx, ty = w["tile_pos_in_workspace_view"]
ox, oy = w["window_offset_in_tile"]
print(round(tx + ox + x + bw / 2), round(ty + oy + y + bh / 2))' "$l"
}
click_item() {
    local xy
    xy=$(item_xy "$1") || return 1
    # shellcheck disable=SC2086
    nclick $xy
}
last_ms() { grep "diagnose: rendered $1 in" "$UI_LOG" | tail -n 1 | sed -E 's/.* in ([0-9]+) ms.*/\1/'; }

# --- Profile ------------------------------------------------------------------------------------------------
touch "$D/slow_diag"
p0=$(renders Profiles)
"$UI" diagnose
check "diagnose: opens" nested_wait 5 panel_up
check "diagnose: profiles drawn" nested_wait 5 more_renders Profiles "$p0"
check "diagnose: focused" nested_wait 3 panel_focused
panel_floats() { [[ $(panel_json | python3 -c 'import json,sys; print(json.load(sys.stdin)["is_floating"])') == True ]]; }
check "diagnose: floats" panel_floats
settle
shot 1-profiles
if [[ -n ${DIAG_ONLY_OPEN:-} ]]; then
    # Breite je Quelle am selben Fenster: Diagnose, Textaktionen, wieder Diagnose
    echo "info size diagnose $(panel_size)"
    key escape
    nested_wait 5 panel_down
    "$UI" actions
    nested_wait 5 panel_up && sleep 0.8 && echo "info size actions $(panel_size)"
    key escape
    nested_wait 5 panel_down
    "$UI" diagnose
    nested_wait 5 panel_up && sleep 0.8 && echo "info size diagnose again $(panel_size)"
    key escape
    exit 0
fi

# --- Profil Unit (2): Units werden gesucht, dann gewählt; zurück -----------------------------------------------
i0=$(renders Items)
key 2
check "unit: items drawn" nested_wait 5 more_renders Items "$i0"
sleep 0.3
shot 1a-unit-searching
check "unit: units listed" nested_wait 5 grep -q 'panel: layout unit:2 ' "$UI_LOG"
settle
shot 1b-unit-list
p1=$(renders Profiles)
key backspace
check "unit: back to the profiles" nested_wait 5 more_renders Profiles "$p1"
settle

# --- Einträge (Profil 1), Sammeln mit Fortschritt ----------------------------------------------------------
i0=$(renders Items)
key 1
check "items drawn" nested_wait 5 more_renders Items "$i0"
settle
shot 2-items
pr0=$(renders Progress)
key return
check "progress drawn" nested_wait 5 more_renders Progress "$pr0"
sleep 1.6
shot 3-progress
v0=$(renders Preview)
check "preview drawn" nested_wait 15 more_renders Preview "$v0"
settle
shot 4-preview
echo "info render Preview $(last_ms Preview) ms"

# --- Feld: Tab, tippen (mit „c“: darf kein Copy auslösen), Enter übernimmt -----------------------------------
c0=$(acts diag_copy) s0=$(acts diag_symptom)
key tab
sleep 0.3
nkeys text:mein space text:geheimhost.lan space text:bootet space text:nicht space text:check
sleep 0.3
shot 5-field-typing
check "field: typed keys do not reach the panel (no copy)" test "$(acts diag_copy)" -eq "$c0"
check "field: nothing applied while typing" test "$(acts diag_symptom)" -eq "$s0"
key return
check "field: Enter applies the symptom" nested_wait 5 more_acts diag_symptom "$s0"
check "field: symptom has its length" bash -c "grep -q 'diag_symptom 38' '$MOCK_LOG'"
settle
shot 6-preview-symptom
# Klick hinein, tippen, Esc: nichts übernommen
s1=$(acts diag_symptom)
check "field: click into it" click_item symptom
sleep 0.3
nkeys text:xyz
sleep 0.2
key escape
settle
check "field: Esc discards (nothing applied)" test "$(acts diag_symptom)" -eq "$s1"
check "field: Esc keeps the panel open" panel_up
shot 7-field-esc

# --- Abschnitt abwählen (Space), Senden, Antwort -------------------------------------------------------------
d0=$(acts diag_deselected)
key down
settle
echo "info render Preview after Down $(last_ms Preview) ms"
key space
check "space leaves a section out" nested_wait 5 more_acts diag_deselected "$d0"
settle
key space
nested_wait 5 more_acts diag_deselected "$((d0 + 1))"
settle
se0=$(renders Send) a0=$(renders Answer)
key return
check "send drawn" nested_wait 5 more_renders Send "$se0"
sleep 1.2
shot 8-sending
check "answer drawn" nested_wait 10 more_renders Answer "$a0"
settle
shot 9-answer
key p
settle
shot 10-answer-values
key p
settle

# --- Beleg: Vorschau mit markierten Zeilen, zurück --------------------------------------------------------------
v1=$(renders Preview)
check "evidence: click" click_item ev:1:2
check "evidence: preview drawn" nested_wait 5 more_renders Preview "$v1"
check "evidence: scrolled" nested_wait 3 grep -q 'diagnose: scrolled to' "$UI_LOG"
settle
shot 11-evidence
a1=$(renders Answer)
key backspace
check "evidence: back to the answer" nested_wait 5 more_renders Answer "$a1"
settle

# --- Prüfrunde: Prüfbefehl wählen und ausführen ------------------------------------------------------------------
k0=$(acts diag_check)
check "check: click selects" click_item check:1
settle
shot 12-check-picked
v2=$(renders Preview)
check "run: click" click_item run
check "run: diag_check" nested_wait 5 more_acts diag_check "$k0"
check "run: new preview" nested_wait 15 more_renders Preview "$v2"
settle
shot 13-round-2

# --- Schließen mit Esc --------------------------------------------------------------------------------------------
cl0=$(acts diag_clear)
key escape
check "Esc closes" nested_wait 5 panel_down
check "closing clears the diagnosis" nested_wait 5 more_acts diag_clear "$cl0"

# --- Ein Panel zur Zeit: diagnose schließt die Textaktionen nur, actions schließt die Diagnose nur ------------------
g0=$(acts act_grab)
"$UI" actions
check "switch: actions opens" nested_wait 5 panel_up
nested_wait 5 more_acts act_grab "$g0"
settle
ac0=$(acts act_clear) dp0=$(acts diag_plan)
"$UI" diagnose
check "switch: diagnose closes the text actions" nested_wait 5 panel_down
check "switch: ... with act_clear" nested_wait 5 more_acts act_clear "$ac0"
sleep 0.5
check "switch: ... and opens no diagnosis" test "$(acts diag_plan)" -eq "$dp0"
"$UI" diagnose
check "switch: diagnose opens again" nested_wait 5 panel_up
nested_wait 5 grep -q 'panel: layout profile:1 ' "$UI_LOG"
settle
cl1=$(acts diag_clear) g1=$(acts act_grab)
"$UI" actions
check "switch: actions closes the diagnosis" nested_wait 5 panel_down
check "switch: ... with diag_clear" nested_wait 5 more_acts diag_clear "$cl1"
sleep 0.5
check "switch: ... and grabs nothing" test "$(acts act_grab)" -eq "$g1"
"$UI" actions
check "switch: actions opens again" nested_wait 5 more_acts act_grab "$g1"
nested_wait 5 panel_up
settle
key escape
nested_wait 5 panel_down

# --- Fehleransicht (Sammeln scheitert) ---------------------------------------------------------------------------
rm -f "$D/slow_diag"
touch "$D/fail_collect"
e0=$(renders Error)
"$UI" diagnose
nested_wait 5 panel_up
nested_wait 5 grep -q 'panel: layout profile:1 ' "$UI_LOG"
settle
key 1
settle
key return
check "error: drawn" nested_wait 5 more_renders Error "$e0"
settle
shot 13b-error
key escape
check "error: Esc closes" nested_wait 5 panel_down
rm -f "$D/fail_collect"

# --- Volles Budget: Dauer des Neuaufbaus ---------------------------------------------------------------------------
rm -f "$D/slow_diag"
touch "$D/big_diag"
"$UI" diagnose
nested_wait 5 panel_up
nested_wait 5 grep -q 'panel: layout profile:1 ' "$UI_LOG"
settle
key 1
settle
v3=$(renders Preview)
key return
check "big: preview drawn" nested_wait 15 more_renders Preview "$v3"
settle
echo "info render Preview (48 KiB) $(last_ms Preview) ms"
for _ in 1 2 3; do
    key down
    sleep 0.6
    echo "info render Preview (48 KiB) after Down $(last_ms Preview) ms"
done
shot 14-big
key escape
nested_wait 5 panel_down

# --- Nichts davon im Log ---------------------------------------------------------------------------------------------
for t in diagmarker geheimhost erika.musterfrau musterfrau bootet "mein " xyz REDACTED "host-1" "user-1"; do
    check "log: no '$t'" bash -c "! grep -q -- '$t' '$UI_LOG'"
done

echo "diag-shots: $pass ok, $fail failed (shots and logs in $OUT)"
((fail == 0))
