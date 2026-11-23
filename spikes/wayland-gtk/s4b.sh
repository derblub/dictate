#!/usr/bin/env bash
# Copyright (C) 2026 Daniel Kurdoghlian
# Pushing Pixels, Vienna <https://pushingpixels.at>
# SPDX-License-Identifier: GPL-3.0-or-later
# Spike S4, Nachtrag: Panel über einem Vollbildfenster (mit und ohne `open-focused true`) und Resize-Shader auf dem
# schwebenden Panel (Größe vom Compositor gesetzt bzw. vom Inhalt getrieben).
# Aufruf: bash spikes/wayland-gtk/s4b.sh [AUSGABEORDNER]
set -uo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$HERE/../../tests/wayland/nested/harness.sh"
OUT="${1:-/home/data/stt-test/logs/wl-spikes/s4b/$(date +%H%M%S)}"
mkdir -p "$OUT"
PROBE="$HERE/target/debug/wl-gtk-probe"
: >"$OUT/results.txt"
res() { echo "$*" | tee -a "$OUT/results.txt"; }
win_id() { nmsg -j windows | python3 -c 'import json,sys; t=sys.argv[1]; print(next((w["id"] for w in json.load(sys.stdin) if w["title"]==t or w["app_id"]==t), ""))' "$1"; }
focused() { nmsg -j focused-window | python3 -c 'import json,sys; w=json.load(sys.stdin); print(w["title"] if w else "none")'; }
wins() { nmsg -j windows | python3 -c 'import json,sys; print([(w["title"],w["is_floating"],w["is_focused"],w["layout"]["window_size"]) for w in json.load(sys.stdin)])'; }
wait_win() { local d=$((SECONDS + 15)); until [[ -n $(win_id "$1") ]]; do ((SECONDS >= d)) && return 1; sleep 0.05; done; }
wait_gone() { local d=$((SECONDS + 15)); until [[ -z $(win_id "$1") ]]; do ((SECONDS >= d)) && return 1; sleep 0.05; done; }
pixels() { magick "$1" -fuzz 8% -fill black +opaque "$2" -fill white -opaque "$2" -format '%[fx:int(mean*w*h)]' info:; }
keep_logs() { local f; for f in "$NESTED_DIR"/*.log "$NESTED_DIR/niri/config.kdl"; do [[ -f $f ]] && cp "$f" "$OUT/$1$(basename "$f")"; done; }
cleanup() { [[ -n ${NESTED_DIR:-} && -d ${NESTED_DIR:-/nonexistent} ]] && { keep_logs "$1"; nested_down; }; }
trap 'cleanup abort-' EXIT

fullscreen_case() { # NAME EXTRA_RULE
    NESTED_MODE=2600x1462 NESTED_EXTRA_KDL="window-rule {
    match app-id=\"^dictate-panel\$\"
    open-floating true
    $2
}" nested_up || return 1
    local fa fp
    mkfifo "$NESTED_DIR"/{a,p}.in
    exec {fa}<>"$NESTED_DIR/a.in" {fp}<>"$NESTED_DIR/p.in"
    nested_spawn targetA "$PROBE" target A <"$NESTED_DIR/a.in"
    wait_win A
    nested_spawn panel "$PROBE" panel <"$NESTED_DIR/p.in"
    sleep 1
    nmsg action fullscreen-window
    sleep 1
    echo show >&"$fp"
    wait_win dictate-panel
    sleep 2
    nested_shot "$OUT/fs-$1.png"
    res "F-$1 fullscreen A, panel shown: focused=$(focused) windows=$(wins)"
    nkeys text:xy
    nflush
    sleep 0.5
    res "F-$1 typed xy: panel text=$(grep -E '^ *[0-9]+ text ' "$NESTED_DIR/panel.log" | tail -1 | sed 's/^ *[0-9]* //') A text=$(grep -E '^ *[0-9]+ text ' "$NESTED_DIR/targetA.log" | tail -1 | sed 's/^ *[0-9]* //')"
    echo close >&"$fp"
    wait_gone dictate-panel
    sleep 1
    res "F-$1 after close: focused=$(focused) A-fullscreen-size=$(wins)"
    exec {fa}>&- {fp}>&-
}

resize_case() {
    cat >"$OUT/anim.kdl" <<'EOF'
animations {
    window-resize {
        duration-ms 3000
        curve "linear"
        custom-shader r#"
            vec4 resize_color(vec3 coords_curr_geo, vec3 size_curr_geo) {
                return vec4(1.0, 1.0, 0.0, 1.0);
            }
        "#
    }
}
EOF
    NESTED_MODE=2600x1462 NESTED_ANIM="$OUT/anim.kdl" NESTED_EXTRA_KDL='window-rule {
    match app-id="^dictate-panel$"
    open-floating true
}' nested_up || return 1
    local fp fa
    mkfifo "$NESTED_DIR"/{a,p}.in
    exec {fa}<>"$NESTED_DIR/a.in" {fp}<>"$NESTED_DIR/p.in"
    nested_spawn targetA "$PROBE" target A <"$NESTED_DIR/a.in"
    wait_win A
    nested_spawn panel "$PROBE" panel <"$NESTED_DIR/p.in"
    echo show >&"$fp"
    wait_win dictate-panel
    sleep 2
    res "R0 panel: $(wins)"
    echo grow 700 600 >&"$fp" # vom Inhalt getrieben
    sleep 0.5
    nested_shot "$OUT/resize-client.png"
    res "R1 client-driven grow: yellow px=$(pixels "$OUT/resize-client.png" '#ffff00') windows=$(wins)"
    sleep 3.5
    nmsg action set-window-height --id "$(win_id dictate-panel)" 800
    sleep 0.5
    nested_shot "$OUT/resize-compositor.png"
    res "R2 compositor set-window-height: yellow px=$(pixels "$OUT/resize-compositor.png" '#ffff00') windows=$(wins)"
    sleep 3.5
    nmsg action set-window-height --id "$(win_id A)" 50%
    sleep 0.5
    nested_shot "$OUT/resize-tiled.png"
    res "R3 control: tiled A set-window-height: yellow px=$(pixels "$OUT/resize-tiled.png" '#ffff00')"
    exec {fa}>&- {fp}>&-
}

fullscreen_case default ""; cleanup F1-
fullscreen_case openfocused "open-focused true"; cleanup F2-
resize_case; cleanup R-
echo "out: $OUT"
