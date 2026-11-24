#!/usr/bin/env bash
# Copyright (C) 2026 Daniel Kurdoghlian
# Pushing Pixels, Vienna <https://pushingpixels.at>
# SPDX-License-Identifier: GPL-3.0-or-later
# Spike S3 (W0): Einfügen über Data-Control im verschachtelten niri.
# Ablauf wie dictate paste: Auswahl eines „Passwortmanagers“ (7 Typen samt x-kde-passwordManagerHint, 5 MB Bild)
# sichern, diktierten Text (mit Hinweis) anbieten, Ctrl+V über die virtuelle Tastatur, zurücklegen, alle Typen
# byteweise vergleichen. Zwei Verlaufs-Stellvertreter laufen mit: `greedy` (holt immer) und `respect` (lässt Inhalte
# mit Hinweis aus). Danach: Auswahl nach dem Ende des Anbieters, diktierter Text ohne Hinweis.
# Aufruf: bash spikes/wayland-paste/s3.sh [AUSGABEORDNER]
set -uo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$HERE/../../tests/wayland/nested/harness.sh"
OUT="${1:-/home/data/stt-test/logs/wl-spikes/s3/$(date +%H%M%S)}"
mkdir -p "$OUT"
CLIP="$HERE/target/debug/clip"
VK="$HERE/../wayland-vkbd/target/debug/vk"
PROBE="$HERE/../wayland-gtk/target/debug/wl-gtk-probe"
[[ -x $CLIP && -x $VK && -x $PROBE ]] || { echo "build first" >&2; exit 2; }
: >"$OUT/results.txt"
res() { echo "$*" | tee -a "$OUT/results.txt"; }
cleanup() {
    if [[ -n ${NESTED_DIR:-} && -d ${NESTED_DIR:-/nonexistent} ]]; then
        local f
        for f in "$NESTED_DIR"/*.log; do cp "$f" "$OUT/$(basename "$f")"; done
        nested_down
    fi
}
trap cleanup EXIT
win_id() { nmsg -j windows | python3 -c 'import json,sys; t=sys.argv[1]; print(next((w["id"] for w in json.load(sys.stdin) if w["title"]==t or w["app_id"]==t), ""))' "$1"; }
wait_win() { local d=$((SECONDS + 20)); until [[ -n $(win_id "$1") ]]; do ((SECONDS >= d)) && return 1; sleep 0.1; done; }
gtk_text() { grep -E '^ *[0-9]+ text ' "$NESTED_DIR/$1.log" | tail -1 | sed 's/^ *[0-9]* text //'; }
wait_log() { local d=$((SECONDS + ${3:-10})); until grep -qE -- "$2" "$NESTED_DIR/$1.log" 2>/dev/null; do ((SECONDS >= d)) && return 1; sleep 0.05; done; }
lines() { wc -l <"$NESTED_DIR/$1.log"; }
since() { tail -n +"$(($2 + 1))" "$NESTED_DIR/$1.log" | sed 's/^ *[0-9]* //' | tr '\n' ';'; }
clipc() { WAYLAND_DISPLAY="$NESTED_NIRI_WL" "$CLIP" "$@"; }

# Anbieter starten: NAME DIR (stdin = FIFO, schließen beendet ihn)
declare -A OFD OPID
offer_start() {
    mkfifo "$NESTED_DIR/$1.in"
    local fd
    exec {fd}<>"$NESTED_DIR/$1.in"
    OFD[$1]=$fd
    WAYLAND_DISPLAY="$NESTED_NIRI_WL" setsid "$CLIP" offer "$2" <"$NESTED_DIR/$1.in" >/dev/null 2>"$NESTED_DIR/$1.log" &
    NESTED_PIDS+=($!)
    NESTED_PGIDS+=($!)
    OPID[$1]=$!
    wait_log "$1" 'offering' || return 1
    sleep 0.3
}
offer_stop() { local fd=${OFD[$1]}; exec {fd}>&-; }

NESTED_MODE=2600x1462 nested_up || exit 1
D="$NESTED_DIR/tmp"

# Testdaten (synthetisch)
PW="$D/pw"
mkdir -p "$PW"
printf 'text/plain;charset=utf-8\ntext/plain\nUTF8_STRING\ntext/html\nimage/png\napplication/x-dictate-test\nx-kde-passwordManagerHint\n' >"$PW/types.txt"
printf 'Pw-Gehe1m!äß' >"$PW/0.bin"
printf 'Pw-Gehe1m!' >"$PW/1.bin"
printf 'Pw-Gehe1m!äß' >"$PW/2.bin"
printf '<b>Pw-Gehe1m!</b>' >"$PW/3.bin"
head -c 5242880 /dev/urandom >"$PW/4.bin"
head -c 65536 /dev/urandom >"$PW/5.bin"
printf 'secret' >"$PW/6.bin"
mkdict() { # DIR HINT(yes|no)
    mkdir -p "$1"
    local t='Diktat: Grüße aus Wien'
    printf 'text/plain;charset=utf-8\ntext/plain\nUTF8_STRING\nTEXT\nSTRING\n' >"$1/types.txt"
    printf '%s' "$t" >"$1/0.bin"; printf '%s' "$t" >"$1/1.bin"; printf '%s' "$t" >"$1/2.bin"
    printf '%s' "$t" >"$1/3.bin"; printf '%s' "$t" >"$1/4.bin"
    if [[ $2 == yes ]]; then echo x-kde-passwordManagerHint >>"$1/types.txt"; printf 'secret' >"$1/5.bin"; fi
}
mkdict "$D/dict" yes
mkdict "$D/dict-nohint" no

# Zielfenster und virtuelle Tastatur
mkfifo "$NESTED_DIR/g.in" "$NESTED_DIR/k.in"
exec {GFD}<>"$NESTED_DIR/g.in" {KFD}<>"$NESTED_DIR/k.in"
nested_spawn gtk "$PROBE" target gtk <"$NESTED_DIR/g.in"
wait_win gtk || exit 1
nested_spawn kbd "$VK" kbd <"$NESTED_DIR/k.in"
wait_log kbd ' ready' || exit 1
vkc() { local n; n=$(grep -cE ' (ok|error) ' "$NESTED_DIR/kbd.log"); echo "$*" >&"$KFD"; local d=$((SECONDS + 10)); until (($(grep -cE ' (ok|error) ' "$NESTED_DIR/kbd.log") > n)); do ((SECONDS >= d)) && return 1; sleep 0.02; done; }

# Verlaufs-Stellvertreter
WAYLAND_DISPLAY="$NESTED_NIRI_WL" setsid "$CLIP" watch greedy </dev/null >/dev/null 2>"$NESTED_DIR/greedy.log" &
NESTED_PIDS+=($!); NESTED_PGIDS+=($!)
WAYLAND_DISPLAY="$NESTED_NIRI_WL" setsid "$CLIP" watch respect </dev/null >/dev/null 2>"$NESTED_DIR/respect.log" &
NESTED_PIDS+=($!); NESTED_PGIDS+=($!)
sleep 0.5

# 1. Passwortmanager setzt die Auswahl
offer_start pw "$PW"
sleep 0.5
res "S0 history stand-ins on password-manager copy: greedy=[$(since greedy 0)] respect=[$(since respect 0)]"
res "S0 types offered: $(clipc types)"

# 2. dictate: sichern
nG=$(lines greedy) nR=$(lines respect)
WAYLAND_DISPLAY="$NESTED_NIRI_WL" "$CLIP" save "$D/saved" 2>"$NESTED_DIR/save.log"
res "S1 save: $(since save 0)"
ok=yes
i=0
while read -r t; do cmp -s "$PW/$i.bin" "$D/saved/$i.bin" || { ok=no; res "S1 differs: $t"; }; i=$((i + 1)); done <"$PW/types.txt"
res "S1 saved bytes equal to original for all $(wc -l <"$PW/types.txt") types: $ok; pw sends during save: $(grep -c 'send #' "$NESTED_DIR/pw.log")"

# 3. dictate: diktierten Text mit Hinweis anbieten, Ctrl+V
nG=$(lines greedy) nR=$(lines respect)
offer_start dict "$D/dict"
sleep 0.6
res "S2 pw source after dictate offer: $(grep -E 'cancelled|ends' "$NESTED_DIR/pw.log" | sed 's/^ *[0-9]* //' | tr '\n' ';')"
res "S2 history on dictated text (hint): greedy=[$(since greedy "$nG")] respect=[$(since respect "$nR")]"
nD=$(lines dict)
nmsg action focus-window --id "$(win_id gtk)"
sleep 0.3
T0=$(grep -cE 'send #' "$NESTED_DIR/dict.log")
vkc "chord ctrl+v"
wait_log gtk 'text ".*Grüße aus Wien' 5 || res "S3 paste did not arrive"
sleep 0.5
res "S3 gtk text after Ctrl+V: $(gtk_text gtk)"
res "S3 sends on dictate source before Ctrl+V: $T0, after: $(since dict "$nD")"

# 4. zurücklegen (gesicherte Typen) und vergleichen
nG=$(lines greedy) nR=$(lines respect)
offer_start restore "$D/saved"
sleep 0.6
res "S4 dict source after restore: $(grep -E 'cancelled|ends' "$NESTED_DIR/dict.log" | sed 's/^ *[0-9]* //' | tr '\n' ';')"
res "S4 history on restore: greedy=[$(since greedy "$nG")] respect=[$(since respect "$nR")]"
res "S4 types after restore: $(clipc types)"
ok=yes
i=0
while read -r t; do
    clipc get "$t" >"$D/back.$i" 2>/dev/null
    cmp -s "$PW/$i.bin" "$D/back.$i" || { ok=no; res "S4 differs: $t ($(stat -c %s "$D/back.$i") bytes)"; }
    i=$((i + 1))
done <"$PW/types.txt"
res "S4 restored bytes equal for all types (incl. 5 MB image): $ok"

# 5. Anbieter beendet sich: bleibt die Auswahl?
nG=$(lines greedy)
kill -TERM "${OPID[restore]}"
local_d=$((SECONDS + 5)); while kill -0 "${OPID[restore]}" 2>/dev/null && ((SECONDS < local_d)); do sleep 0.05; done
kill -0 "${OPID[restore]}" 2>/dev/null && res "S5 restore offer still alive"
sleep 0.8
res "S5 after restoring process ended: types=$(clipc types); history: greedy=[$(since greedy "$nG")]"

# 6. diktierter Text ohne Hinweis
nG=$(lines greedy) nR=$(lines respect)
offer_start dictnh "$D/dict-nohint"
sleep 0.6
res "S6 history on dictated text WITHOUT hint: greedy=[$(since greedy "$nG")] respect=[$(since respect "$nR")]"
offer_stop dictnh
offer_stop dict 2>/dev/null
offer_stop pw 2>/dev/null
res "done"
