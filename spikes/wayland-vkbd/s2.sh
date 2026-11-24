#!/usr/bin/env bash
# Copyright (C) 2026 Daniel Kurdoghlian
# Pushing Pixels, Vienna <https://pushingpixels.at>
# SPDX-License-Identifier: GPL-3.0-or-later
# Spike S2 (W0): eigene virtuelle Tastatur (T2) und Eingabemethode (T3) im verschachtelten niri.
# Aufruf: bash spikes/wayland-vkbd/s2.sh [AUSGABEORDNER]
# Ziele: GTK-4-Eingabefeld (wl-gtk-probe target), Alacritty (nativ), xterm (XWayland über xwayland-satellite des
# verschachtelten niri); Browser in s2b.sh. Terminals schreiben die empfangenen Bytes roh in eine Datei.
set -uo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$HERE/../../tests/wayland/nested/harness.sh"
OUT="${1:-/home/data/stt-test/logs/wl-spikes/s2/$(date +%H%M%S)}"
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

# ---------------------------------------------------------------- T2: Text tippen
vk_start k1 kbd debug
for t in gtk alacritty xterm; do
    [[ -n $(win_id $t) ]] || continue
    focus $t
    vk k1 "type $TEXT"
    sleep 0.8
done
res "T2-gtk:       $(gtk_text gtk)"
res "T2-alacritty: $(raw "$OUT/alacritty.bin")"
res "T2-xterm:     $(raw "$OUT/xterm.bin")"
res "T2 expected:  '$TEXT'"

# Belegung danach: physische Taste y (us-Code 21) muss im de-Layout des niri wieder 'z' ergeben
focus gtk
nkeys text:y
nflush
sleep 0.5
res "T2-keymap-restore gtk after physical key 21 (de => 'z'): $(gtk_text gtk); keymap events at gtk: $(grep -cE 'wl_keyboard#[0-9]+\.keymap\(' "$NESTED_DIR/gtk.log")"
for t in alacritty xterm; do
    [[ -n $(win_id $t) ]] || continue
    before=$(rawlen "$OUT/$t.bin")
    focus $t
    nkeys text:y
    nflush
    sleep 0.5
    res "T2-keymap-restore $t: new bytes $(python3 -c 'import sys; b=open(sys.argv[1],"rb").read()[int(sys.argv[2]):]; print(repr(b))' "$OUT/$t.bin" "$before")"
done

# ---------------------------------------------------------------- gehaltener Modifikator (Mod = Alt verschachtelt)
focus gtk
# Positivkontrolle: dieselben Kürzel über die physische Tastatur müssen auslösen.
rm -f "$MARK"/*
nkeys alt+q
nflush
sleep 0.8
nkeys alt+shift+q
nflush
sleep 0.8
res "H0 control, physical alt+q and alt+shift+q: bind markers=[$(marks)]"
rm -f "$MARK"/*
vk k1 "chord alt+shift+q"
sleep 0.8
res "H1 vkbd-only chord alt+shift+q: bind markers=[$(marks)] gtk text=$(gtk_text gtk)"
vk k1 "chord alt+q"
sleep 0.8
res "H1b vkbd-only chord alt+q: bind markers=[$(marks)]"
rm -f "$MARK"/*
n0=$(wc -l <"$NESTED_DIR/gtk.log")
nkeys down:alt f24
sleep 0.3
vk k1 "type w"
sleep 0.5
nkeys up:alt f24
nflush
sleep 0.5
res "H2 physical Alt held + vkbd plain 'w': bind markers=[$(marks)] gtk text=$(gtk_text gtk)"
res "H3 gtk wire during H2: $(tail -n +"$n0" "$NESTED_DIR/gtk.log" | grep -oE 'wl_keyboard#[0-9]+\.(key|modifiers|keymap)\([^)]*\)' | sed 's/wl_keyboard#[0-9]*\.//' | tr '\n' ' ')"
res "H3b gtk events during H2: $(tail -n +"$n0" "$NESTED_DIR/gtk.log" | grep -E '^ *[0-9]+ (key|modifiers|text)' | sed 's/^ *[0-9]* //' | tr '\n' ';')"
rm -f "$MARK"/*

# ---------------------------------------------------------------- Abbruch mitten im Anschlag
# A1 Basis: down/up. A2: down, dann destroy (Prozess lebt). A3: down, dann Prozessende ohne Loslassen.
focus gtk
vk k1 "type \n" >/dev/null
n0=$(wc -l <"$NESTED_DIR/gtk.log")
vk k1 "down a"
vk k1 "up a"
sleep 1.5
res "A1 down/up 'a': gtk text=$(gtk_text gtk)"
vk_start k2 kbd debug
n0=$(wc -l <"$NESTED_DIR/gtk.log")
vk k2 "down b"
vk k2 "destroy"
sleep 2
res "A2 down 'b' then destroy: gtk text=$(gtk_text gtk); wire at gtk: $(tail -n +"$n0" "$NESTED_DIR/gtk.log" | grep -oE 'wl_keyboard#[0-9]+\.(key|keymap|leave|enter)\([^)]*\)' | sed 's/wl_keyboard#[0-9]*\.//' | sort | uniq -c | tr -s ' ' | tr '\n' ';')"
vk_start k3 kbd
n0=$(wc -l <"$NESTED_DIR/gtk.log")
vk k3 "down c"
vk_raw k3 "exit"
sleep 2
res "A3 down 'c' then process exit: gtk text=$(gtk_text gtk); wire at gtk: $(tail -n +"$n0" "$NESTED_DIR/gtk.log" | grep -oE 'wl_keyboard#[0-9]+\.(key|keymap|leave|enter)\([^)]*\)' | sed 's/wl_keyboard#[0-9]*\.//' | sort | uniq -c | tr -s ' ' | tr '\n' ';')"
for t in alacritty xterm; do
    [[ -n $(win_id $t) ]] || continue
    focus $t
    before=$(rawlen "$OUT/$t.bin")
    vk_start "k4$t" kbd
    vk "k4$t" "down d"
    vk_raw "k4$t" "exit"
    sleep 2
    res "A3-$t down 'd' then process exit: new bytes $(python3 -c 'import sys; b=open(sys.argv[1],"rb").read()[int(sys.argv[2]):]; print(repr(b))' "$OUT/$t.bin" "$before")"
    before=$(rawlen "$OUT/$t.bin")
    vk_start "k5$t" kbd
    vk "k5$t" "down e"
    vk "k5$t" "destroy"
    sleep 2
    res "A2-$t down 'e' then destroy: new bytes $(python3 -c 'import sys; b=open(sys.argv[1],"rb").read()[int(sys.argv[2]):]; print(repr(b))' "$OUT/$t.bin" "$before")"
done

# ---------------------------------------------------------------- T3: Eingabemethode
focus gtk
vk_start im1 im
sleep 0.5
res "I1 IM created while gtk focused: events=$(grep -E ' (activate|deactivate|done|unavailable|content_type)' "$NESTED_DIR/im1.log" | sed 's/^ *[0-9]* //' | tr '\n' ';')"
focus alacritty
focus gtk
sleep 0.3
res "I1b after refocus gtk: events=$(grep -E ' (activate|deactivate|unavailable)' "$NESTED_DIR/im1.log" | sed 's/^ *[0-9]* //' | tr '\n' ';')"
vk im1 "commit $TEXT"
sleep 0.5
res "I2 IM commit into gtk: $(gtk_text gtk) [$(grep -E ' committed' "$NESTED_DIR/im1.log" | tail -1 | sed 's/^ *[0-9]* //')]"
nkeys text:abc
nflush
sleep 0.5
res "I3 physical typing while IM bound: gtk=$(gtk_text gtk)"
vk_start im2 im
sleep 0.5
res "I4 second IM: $(grep -E ' (activate|unavailable|error)' "$NESTED_DIR/im2.log" | sed 's/^ *[0-9]* //' | tr '\n' ';')"
nkeys down:alt f24
sleep 0.3
vk im1 "commit q"
sleep 0.5
nkeys up:alt f24
nflush
sleep 0.4
res "I5 IM commit 'q' while physical Alt held: markers=[$(marks)] gtk=$(gtk_text gtk)"
for t in alacritty xterm; do
    [[ -n $(win_id $t) ]] || continue
    n0=$(wc -l <"$NESTED_DIR/im1.log")
    focus $t
    sleep 0.3
    before=$(rawlen "$OUT/$t.bin")
    vk im1 "commit $TEXT"
    sleep 0.6
    res "I6-$t IM events on focus: $(tail -n +"$n0" "$NESTED_DIR/im1.log" | grep -E ' (activate|deactivate|content_type|committed)' | sed 's/^ *[0-9]* //' | tr '\n' ';')"
    res "I6-$t IM commit bytes: $(python3 -c 'import sys; b=open(sys.argv[1],"rb").read()[int(sys.argv[2]):]; print(repr(b.decode("utf-8","replace")))' "$OUT/$t.bin" "$before")"
done
res "done"
