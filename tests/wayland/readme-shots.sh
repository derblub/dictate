#!/usr/bin/env bash
# Copyright (C) 2026 Daniel Kurdoghlian
# Pushing Pixels, Vienna <https://pushingpixels.at>
# SPDX-License-Identifier: GPL-3.0-or-later
# Bilder für die README (PLAN-lizenz.md U5c): dictate-ui im verschachtelten niri gegen den Demo-Daemon
# (tests/wayland/demo_daemon.py: der Mock mit englischen Beispielinhalten), je Ansicht ein Screenshot und ein
# Ausschnitt um Overlay bzw. Panel. Manuell, kein Prüfschritt; die Bilder in docs/assets wählt ein Mensch aus.
#
# Aufruf: tests/wayland/readme-shots.sh [AUSGABEORDNER]  (nach `cargo build --release --bin dictate-ui` und dem
#         Injektor; Vorgabe $DICTATE_TEST_ROOT/logs/readme-shots). Mit DICTATE_PALETTE=DATEI eine andere Palette.
# Isolation wie run.sh: tests/wayland/nested/harness.sh, eigener Laufzeitordner, nie die echte Sitzung.
set -uo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
UI="$ROOT/target/release/dictate-ui"
[[ -x $UI ]] || { echo "FAILED: $UI is not built"; exit 1; }
[[ -x $HERE/nested/target/debug/nested-inject ]] || { echo "FAILED: injector not built (tests/wayland/nested)"; exit 1; }

# shellcheck source=nested/harness.sh
source "$HERE/nested/harness.sh"
OUT="${1:-${DICTATE_TEST_ROOT:-}/logs/readme-shots}"
[[ -n $WL_RUN_BASE ]] || { echo "FAILED: no usable test folder (DICTATE_TEST_ROOT, message above)"; exit 1; }

fail=0
bad() { echo "FAIL $*"; fail=$((fail + 1)); }

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
unset DICTATE_SOCKET DICTATE_UI_SOCKET
export NESTED_MODE=2600x1950 NESTED_LAYOUT=us NESTED_VARIANT=""
export NESTED_EXTRA_KDL="include \"$ROOT/niri/dictate-binds.kdl\""
nested_up || { echo "FAILED: nested niri did not come up"; exit 1; }
[[ -n $WL_RUN_BASE && $XDG_RUNTIME_DIR == "$WL_RUN_BASE"/* ]] || { echo "FAILED: XDG_RUNTIME_DIR outside $WL_RUN_BASE"; exit 1; }
[[ ${DBUS_SYSTEM_BUS_ADDRESS:-} == "unix:path=$NESTED_RT/"* ]] || { echo "FAILED: not the private test bus"; exit 1; }
D=$NESTED_DIR
MOCK_LOG=$D/mock.log
UI_LOG=$D/ui.log
: >"$MOCK_LOG"
echo 0 >"$D/keys"
mkdir -p "$NESTED_RT/dictate" && chmod 700 "$NESTED_RT/dictate"
python3 "$HERE/demo_daemon.py" "$NESTED_RT/dictate/control.sock" "$MOCK_LOG" 120 >"$D/mock-out.log" 2>&1 &
MOCK_PID=$!
nested_wait 5 test -S "$NESTED_RT/dictate/control.sock" || { echo "FAILED: demo daemon"; exit 1; }
nested_spawn logind env DBUS_SESSION_BUS_ADDRESS="$DBUS_SYSTEM_BUS_ADDRESS" FAKE_LOGIND_LOG="$D/logind.calls" python3 "$HERE/fake-logind"
nested_wait 5 grep -q ready "$D/logind.log" || { echo "FAILED: fake logind"; exit 1; }
# Programme der Palette nur aus einem leeren Ordner: hier wird nichts gestartet, nie der echte Suchpfad
mkdir -p "$D/fakes"
nested_spawn ui env DICTATE_LOG=dictate_ui=debug,info NIRI_SOCKET="$NESTED_NIRI_SOCKET" XDG_SESSION_ID=2 "$UI" run --fake-keys "$D/keys" --fake-programs "$D/fakes"
nested_wait 10 test -S "$NESTED_RT/dictate/ui.sock" || { echo "FAILED: dictate-ui"; exit 1; }
nested_wait 5 grep -q 'connected to the daemon' "$UI_LOG" || { echo "FAILED: dictate-ui not connected"; exit 1; }
nested_wait 10 grep -q 'renderer ready' "$UI_LOG"

# Zeiger in die Ecke unten rechts: liegt so in keinem Ausschnitt
ex=${NESTED_EXTENT%,*} ey=${NESTED_EXTENT#*,}
nkeys "move:$((ex - 4)),$((ey - 4))" sleep:100 f24 sleep:100

win_id() { nmsg -j windows | python3 -c 'import json,sys; a=sys.argv[1]; print(next((w["id"] for w in json.load(sys.stdin) if w["app_id"]==a), ""))' "$1"; }
has_win() { [[ -n $(win_id "$1") ]]; }
focused_is() { [[ $(nmsg -j focused-window | python3 -c 'import json,sys; print((json.load(sys.stdin) or {}).get("id"))') == "$1" ]]; }
focus() { local id; id=$(win_id "$1"); [[ -n $id ]] && nmsg action focus-window --id "$id" && nested_wait 3 focused_is "$id"; }
panel_json() { nmsg -j windows | python3 -c 'import json,sys
w=[w for w in json.load(sys.stdin) if w.get("app_id")=="dictate-panel"]
print(json.dumps(w[0]) if w else "")'; }
panel_up() { [[ -n $(panel_json) ]]; }
panel_down() { ! panel_up; }
acts() { grep -c " $1" "$MOCK_LOG"; }
more_acts() { (($(acts "$1") > $2)); }
renders() { grep -c "diagnose: rendered $1 in" "$UI_LOG"; }
more_renders() { (($(renders "$1") > $2)); }
key() { nkeys "$1" && sleep 0.05; }

# Ausschnitt um das Panel: Kachel aus der niri-IPC (logisch) mal Skalierung, dazu ein Rand
crop_panel() { # NAME
    local box
    box=$(panel_json | python3 -c 'import json,sys
w = json.loads(sys.stdin.read())["layout"]
s, m = float(sys.argv[1]), 40
x, y = w["tile_pos_in_workspace_view"]
bw, bh = w["tile_size"]
print(*(round(v * s) for v in (x - m, y - m, x + bw + m, y + bh + m)))' "$NESTED_SCALE") || return 1
    nested_shot "$D/full-$1.png" || return 1
    python3 - "$D/full-$1.png" "$D/$1.png" $box <<'PY'
import sys
from PIL import Image
src, dst, *box = sys.argv[1:]
Image.open(src).convert("RGB").crop(tuple(int(v) for v in box)).save(dst, optimize=True)
PY
    echo "shot $1.png"
}
# Ausschnitt um das Overlay (Layer-Fläche, nicht in der niri-IPC): Unterschied zu einem Bild ohne Overlay
# (erst, wenn es mindestens MIN Pixel breit ist: mit der Mitschrift wächst es)
crop_diff_once() { # NAME REFERENZ MIN
    nested_shot "$D/full-$1.png" || return 1
    python3 - "$D/full-$1.png" "$2" "$D/$1.png" "$3" <<'PY'
import sys
from PIL import Image, ImageChops
a = Image.open(sys.argv[1]).convert("RGB")
b = Image.open(sys.argv[2]).convert("RGB")
box = ImageChops.difference(a, b).convert("L").point(lambda v: 255 if v > 24 else 0).getbbox()
if not box or box[2] - box[0] < int(sys.argv[4]):
    sys.exit(1)
x0, y0, x1, y1 = box
m = 64
a.crop((max(0, x0 - m), max(0, y0 - m), min(a.width, x1 + m), min(a.height, y1 + m))).save(sys.argv[3], optimize=True)
PY
}
crop_diff() { # NAME REFERENZ MIN
    nested_wait 8 crop_diff_once "$@" || return 1
    echo "shot $1.png"
}

# --- 1. Overlay beim Sprechen über einem leeren Fenster --------------------------------------------------------------
nested_spawn backdrop python3 "$HERE/target.py"
nested_wait 10 has_win dictate-test-target || { echo "FAILED: backdrop window"; exit 1; }
nmsg action maximize-column
sleep 0.5
nested_shot "$D/ref-overlay.png"
s0=$(acts start)
echo 1 >"$D/keys"
key alt+e
nested_wait 3 more_acts start "$s0" || bad "overlay: start"
sleep 0.8
crop_diff overlay "$D/ref-overlay.png" 1000 || bad "overlay: shot"
echo 0 >"$D/keys"
nflush
sleep 1.0

# --- 2. Befehlspalette mit Eingabe „fi“ --------------------------------------------------------------------------------
"$UI" palette
nested_wait 5 panel_up || bad "palette: opens"
sleep 0.6
key text:fi
nested_wait 5 grep -q 'pal_resolve 2' "$MOCK_LOG" || bad "palette: resolve"
sleep 0.8
crop_panel palette || bad "palette: shot"
key escape
nested_wait 5 panel_down || bad "palette: closes"

# --- 3. Textaktion „Explain“ auf einer markierten Fehlermeldung ----------------------------------------------------------
SEL="ModuleNotFoundError: No module named 'requests'"
nested_spawn sel python3 "$HERE/target.py" text dictate-demo-editor "$SEL"
nested_wait 10 has_win dictate-demo-editor || bad "actions: editor window"
nested_wait 5 grep -q '^selected' "$D/sel.log"
focus dictate-demo-editor || bad "actions: focus editor"
nmsg action maximize-column
sleep 0.5
g0=$(acts act_grab)
key alt+i
nested_wait 5 panel_up || bad "actions: panel opens"
nested_wait 5 more_acts act_grab "$g0" || bad "actions: grab"
sleep 0.8
crop_panel actions-choose || bad "actions: shot of the actions"
r0=$(acts act_run)
key e
nested_wait 5 more_acts act_run "$r0" || bad "actions: run"
nested_wait 5 test -s "$D/result.txt" || bad "actions: answer complete"
sleep 1.0
crop_panel actions-answer || bad "actions: shot of the answer"
key escape
nested_wait 5 panel_down || bad "actions: closes"

# --- 4. Diagnose: Profil „Last boot“, Vorschau, Antwort ------------------------------------------------------------------
p0=$(renders Profiles)
"$UI" diagnose
nested_wait 5 panel_up || bad "diagnose: opens"
nested_wait 5 more_renders Profiles "$p0" || bad "diagnose: profiles"
sleep 0.5
crop_panel diagnose-profiles || bad "diagnose: shot of the profiles"
i0=$(renders Items)
key 1
nested_wait 5 more_renders Items "$i0" || bad "diagnose: items"
v0=$(renders Preview)
key return
nested_wait 15 more_renders Preview "$v0" || bad "diagnose: preview"
sleep 0.8
crop_panel diagnose-preview || bad "diagnose: shot of the preview"
a0=$(acts diag_send)
key return
nested_wait 10 more_acts diag_send "$a0" || bad "diagnose: send"
nested_wait 10 grep -q 'diagnose: rendered Answer in' "$UI_LOG" || bad "diagnose: answer"
sleep 1.0
crop_panel diagnose-answer || bad "diagnose: shot of the answer"
key escape
nested_wait 5 panel_down || bad "diagnose: closes"

echo "== $fail problem(s), pictures in $OUT =="
[[ $fail -eq 0 ]]
