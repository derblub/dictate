#!/usr/bin/env bash
# Copyright (C) 2026 Daniel Kurdoghlian
# Pushing Pixels, Vienna <https://pushingpixels.at>
# SPDX-License-Identifier: GPL-3.0-or-later
# Spike S2, Nachtrag: gehaltene physische Modifikatoren (Ctrl, Alt) beim Tippen in Terminals, Zeilenumbruch über
# die Eingabemethode, zweite Eingabemethode nach Ende der ersten, Zeit bis activate.
# Aufruf: bash spikes/wayland-vkbd/s2c.sh [AUSGABEORDNER]
# Ziele: GTK-4-Eingabefeld (wl-gtk-probe target), Alacritty (nativ), xterm (XWayland über xwayland-satellite des
# verschachtelten niri); Browser in s2b.sh. Terminals schreiben die empfangenen Bytes roh in eine Datei.
set -uo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$HERE/../../tests/wayland/nested/harness.sh"
OUT="${1:-/home/data/stt-test/logs/wl-spikes/s2c/$(date +%H%M%S)}"
mkdir -p "$OUT"
VK="$HERE/target/debug/vk"
PROBE="$HERE/../wayland-gtk/target/debug/wl-gtk-probe"
[[ -x $VK && -x $PROBE ]] || { echo "build first" >&2; exit 2; }
: >"$OUT/results.txt"
res() { echo "$*" | tee -a "$OUT/results.txt"; }
TEXT='Grüße, Maß: äöü ÄÖÜ ß € – „ok“'

cat >"$OUT/rawcat.py" <<'EOF'
import os, sys, tty
tty.setraw(0)
with open(sys.argv[1], "ab", 0) as f:
    while True:
        b = os.read(0, 4096)
        if not b:
            break
        f.write(b)
EOF

MARK="$(mktemp -d "$WL_RUN_BASE/m.XXXXXX")"
cleanup() {
    if [[ -n ${NESTED_DIR:-} && -d ${NESTED_DIR:-/nonexistent} ]]; then
        local f
        for f in "$NESTED_DIR"/*.log; do cp "$f" "$OUT/$(basename "$f")"; done
        nested_down
    fi
    rm -rf "$MARK"
}
trap cleanup EXIT

win_id() { nmsg -j windows | python3 -c 'import json,sys; t=sys.argv[1]; print(next((w["id"] for w in json.load(sys.stdin) if w["title"]==t or w["app_id"]==t), ""))' "$1"; }
focused() { nmsg -j focused-window | python3 -c 'import json,sys; w=json.load(sys.stdin); print(w["title"] if w else "none")'; }
wait_win() { local d=$((SECONDS + ${2:-20})); until [[ -n $(win_id "$1") ]]; do ((SECONDS >= d)) && { echo "timeout window $1" >&2; return 1; }; sleep 0.1; done; }
focus() { nmsg action focus-window --id "$(win_id "$1")"; sleep 0.3; }
gtk_text() { grep -E '^ *[0-9]+ text ' "$NESTED_DIR/$1.log" | tail -1 | sed 's/^ *[0-9]* text //'; }
raw() { python3 -c 'import sys; b=open(sys.argv[1],"rb").read(); print(repr(b.decode("utf-8","replace")), len(b), "bytes")' "$1" 2>/dev/null || echo "(no file)"; }
rawlen() { stat -c %s "$1" 2>/dev/null || echo 0; }
marks() { ls "$MARK" 2>/dev/null | tr '\n' ' '; }

# vk-Instanzen: NAME MODE -> FIFO NESTED_DIR/NAME.in, Log NAME.log
declare -A VKFD
vk_start() { # NAME MODE [WAYLAND_DEBUG]
    mkfifo "$NESTED_DIR/$1.in"
    local fd
    exec {fd}<>"$NESTED_DIR/$1.in"
    VKFD[$1]=$fd
    if [[ ${3:-} == debug ]]; then
        WAYLAND_DEBUG=client nested_spawn "$1" "$VK" "$2" <"$NESTED_DIR/$1.in"
    else
        nested_spawn "$1" "$VK" "$2" <"$NESTED_DIR/$1.in"
    fi
    local d=$((SECONDS + 10))
    until grep -qE ' (ready|error)' "$NESTED_DIR/$1.log" 2>/dev/null; do ((SECONDS >= d)) && return 1; sleep 0.05; done
    sleep 0.4 # Clients binden wl_keyboard erst nach der neuen Fähigkeit des Sitzes
}
vk() { # NAME BEFEHL… -> wartet auf ok/error
    local n
    n=$(grep -cE ' (ok|error) ' "$NESTED_DIR/$1.log")
    echo "${*:2}" >&"${VKFD[$1]}"
    local d=$((SECONDS + 10))
    until (($(grep -cE ' (ok|error) ' "$NESTED_DIR/$1.log") > n)); do
        ((SECONDS >= d)) && { echo "vk $1 timeout" >&2; return 1; }
        sleep 0.02
    done
}
vk_raw() { echo "${*:2}" >&"${VKFD[$1]}"; } # ohne Warten (für exit)

NESTED_XWAYLAND=1 NESTED_MODE=2600x1462 NESTED_EXTRA_KDL="binds {
    Mod+Shift+Q { spawn \"touch\" \"$MARK/mod-shift-q\"; }
    Mod+Q { spawn \"touch\" \"$MARK/mod-q\"; }
    Mod+W { spawn \"touch\" \"$MARK/mod-w\"; }
}" nested_up || exit 1

# X-Display des verschachtelten niri (xwayland-satellite, bei Bedarf gestartet)
XDISP="$(sed 's/\x1b\[[0-9;]*m//g' "$NESTED_DIR/niri.log" | grep -oE 'listening on X11 socket: :[0-9]+' | grep -oE ':[0-9]+$' | head -1)"
res "X display of nested niri: ${XDISP:-none} ($(grep -iE 'xwayland|x11' "$NESTED_DIR/niri.log" | sed 's/\x1b\[[0-9;]*m//g' | cut -c1-160 | head -2 | tr '\n' '|'))"

# Ziele starten
mkfifo "$NESTED_DIR/g.in"
exec {GFD}<>"$NESTED_DIR/g.in"
WAYLAND_DEBUG=client nested_spawn gtk "$PROBE" target gtk <"$NESTED_DIR/g.in"
wait_win gtk || exit 1
nested_spawn alacritty alacritty --class t-alacritty -T alacritty -e python3 -I "$OUT/rawcat.py" "$OUT/alacritty.bin" </dev/null
wait_win alacritty || exit 1
if [[ -n $XDISP && $XDISP != :0 ]]; then
    DISPLAY="$XDISP" LC_ALL=en_US.UTF-8 nested_spawn xterm xterm -u8 -T xterm -e python3 -I "$OUT/rawcat.py" "$OUT/xterm.bin" </dev/null
    wait_win xterm 30 || res "xterm did not map"
else
    res "xterm skipped: no safe X display (${XDISP:-none})"
fi
sleep 1
nmsg -j windows >"$OUT/windows.json"


newbytes() { python3 -c 'import sys; b=open(sys.argv[1],"rb").read()[int(sys.argv[2]):]; print(repr(b))' "$1" "$2"; }
vk_start k1 kbd
for t in alacritty xterm gtk; do
    [[ -n $(win_id $t) ]] || continue
    for m in ctrl alt; do
        focus $t
        before=$(rawlen "$OUT/$t.bin")
        nkeys "down:$m" f24
        sleep 0.3
        vk k1 "type c"
        sleep 0.3
        nkeys "up:$m" f24
        nflush
        sleep 0.4
        if [[ $t == gtk ]]; then
            res "H4-$t physical $m held + vkbd 'c': gtk text=$(gtk_text gtk)"
        else
            res "H4-$t physical $m held + vkbd 'c': new bytes $(newbytes "$OUT/$t.bin" "$before") (F24 = \\x1b[45~ or similar)"
        fi
    done
done
for t in alacritty gtk; do
    focus $t
    before=$(rawlen "$OUT/$t.bin")
    t0=$(date +%s%3N)
    vk_start "im$t" im
    sleep 0.3
    res "I8-$t IM start->activate: $(grep -m1 ' activate' "$NESTED_DIR/im$t.log" | awk '{print $1}') ms after process start (process start took $(( $(date +%s%3N) - t0 )) ms incl. 0.7 s waits)"
    vk "im$t" 'commit x\ny\r\nz'
    sleep 0.5
    if [[ $t == gtk ]]; then res "I7-$t commit 'x\\ny\\r\\nz': gtk text=$(gtk_text gtk)"; else res "I7-$t commit 'x\\ny\\r\\nz': bytes $(newbytes "$OUT/$t.bin" "$before")"; fi
    vk_start "im2$t" im
    sleep 0.3
    vk "im$t" destroy
    sleep 0.3
    focus alacritty; focus $t
    sleep 0.3
    res "I4b-$t second IM after first destroyed: $(grep -E ' (activate|deactivate|unavailable)' "$NESTED_DIR/im2$t.log" | sed 's/^ *[0-9]* //' | tr '\n' ';')"
    vk "im2$t" destroy
done
res "done"
