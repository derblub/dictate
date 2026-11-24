#!/usr/bin/env bash
# Copyright (C) 2026 Daniel Kurdoghlian
# Pushing Pixels, Vienna <https://pushingpixels.at>
# SPDX-License-Identifier: GPL-3.0-or-later
# Spike S4 (W0): GTK-Overlay als Layer-Fläche und Panel als schwebendes Fenster im verschachtelten niri.
# Aufruf: bash spikes/wayland-gtk/s4.sh [AUSGABEORDNER]   (Vorgabe /home/data/stt-test/logs/wl-spikes/s4/<zeit>)
# Teil A: Overlay (Fokus, Klick-durch, Tippen, Skalierung), Panel (schwebend, Fokus, Fokus zurück, Übersicht, MRU).
# Teil B: Shader (eigene Test-Shader und Daniels animations.kdl). Teil C: Variante C aus 3.2 (Tastatur greifen).
set -uo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$HERE/../../tests/wayland/nested/harness.sh"
OUT="${1:-/home/data/stt-test/logs/wl-spikes/s4/$(date +%H%M%S)}"
mkdir -p "$OUT"
PROBE="$HERE/target/debug/wl-gtk-probe"
[[ -x $PROBE ]] || { echo "build first" >&2; exit 2; }
: >"$OUT/results.txt"
res() { echo "$*" | tee -a "$OUT/results.txt"; }

PANEL_RULE='window-rule {
    match app-id="^dictate-panel$"
    open-floating true
    default-column-width { fixed 720; }
    default-window-height { fixed 420; }
}'

win_id() { nmsg -j windows | python3 -c 'import json,sys; t=sys.argv[1]; print(next((w["id"] for w in json.load(sys.stdin) if w["title"]==t or w["app_id"]==t), ""))' "$1"; }
focused() { nmsg -j focused-window | python3 -c 'import json,sys; w=json.load(sys.stdin); print(w["title"] if w else "none")'; }
floating() { nmsg -j windows | python3 -c 'import json,sys; t=sys.argv[1]; print(next((w["is_floating"] for w in json.load(sys.stdin) if w["app_id"]==t), "absent"))' "$1"; }
wait_win() { # APP_ID|TITEL present|absent [SEK]
    local deadline=$((SECONDS + ${3:-15})) got
    while :; do
        got=$([[ -n $(win_id "$1") ]] && echo present || echo absent)
        [[ $got == "$2" ]] && return 0
        ((SECONDS >= deadline)) && { echo "timeout: window $1 not $2" >&2; return 1; }
        sleep 0.05
    done
}
settle() { # warten, bis sich der Fokus 0,6 s nicht mehr ändert (höchstens 10 s)
    local last cur n=0 deadline=$((SECONDS + 10))
    last=$(focused)
    while ((SECONDS < deadline)); do
        sleep 0.2
        cur=$(focused)
        if [[ $cur == "$last" ]]; then ((++n >= 3)) && break; else n=0; last=$cur; fi
    done
}
show_panel() { echo show >&"$fp"; wait_win dictate-panel present; settle; }
hide_panel() { echo "$1" >&"$fp"; wait_win dictate-panel absent; settle; }
wait_line() { # LOG ERE [SEK]
    local deadline=$((SECONDS + ${3:-10}))
    until grep -qE -- "$2" "$NESTED_DIR/$1.log" 2>/dev/null; do
        ((SECONDS >= deadline)) && { echo "timeout: /$2/ in $1" >&2; return 1; }
        sleep 0.05
    done
}
count() { grep -cE -- "$2" "$NESTED_DIR/$1.log" 2>/dev/null || true; }
keep_logs() { local f; for f in "$NESTED_DIR"/*.log "$NESTED_DIR/niri/config.kdl"; do [[ -f $f ]] && cp "$f" "$OUT/${1:-}$(basename "$f")"; done; }
pixels() { # PNG FARBE (#rrggbb) -> Anzahl Pixel dieser Farbe (Toleranz 8 %)
    magick "$1" -fuzz 8% -fill black +opaque "$2" -fill white -opaque "$2" -format '%[fx:int(mean*w*h)]' info:
}

# ------------------------------------------------------------------ Teil A
partA() {
    NESTED_MODE=2600x1462 NESTED_EXTRA_KDL="$PANEL_RULE" nested_up || return 1
    local fa fb fp fo
    mkfifo "$NESTED_DIR"/{a,b,o,p}.in
    exec {fa}<>"$NESTED_DIR/a.in" {fb}<>"$NESTED_DIR/b.in" {fo}<>"$NESTED_DIR/o.in" {fp}<>"$NESTED_DIR/p.in"
    nested_spawn targetA "$PROBE" target A <"$NESTED_DIR/a.in"
    wait_line targetA 'mapped' || return 1
    nested_spawn targetB "$PROBE" target B <"$NESTED_DIR/b.in"
    wait_line targetB 'mapped' || return 1
    # Panel-Prozess langlebig, ohne Fenster (wie dictate-ui); WAYLAND_DEBUG für Skalierung und Aktivierung.
    WAYLAND_DEBUG=client nested_spawn panel "$PROBE" panel <"$NESTED_DIR/p.in"
    wait_line panel 'ready' || return 1
    local A B
    A=$(win_id A) B=$(win_id B)
    res "A1 windows: A=$A B=$B (B newest), focused=$(focused)"
    nmsg action focus-window --id "$A"
    sleep 0.3
    res "A2 focus A before overlay: focused=$(focused)"

    WAYLAND_DEBUG=client nested_spawn overlay "$PROBE" overlay --region empty --keyboard none <"$NESTED_DIR/o.in"
    wait_line overlay 'region empty' || return 1
    sleep 0.3
    nmsg -j layers >"$OUT/A-layers.json"
    res "A3 layers: $(python3 -c 'import json,sys; print([(l["namespace"],l["layer"],l["keyboard_interactivity"]) for l in json.load(open(sys.argv[1]))])' "$OUT/A-layers.json")"
    res "A4 focus after overlay map: focused=$(focused); overlay focus-in lines=$(count overlay 'focus in|active true')"
    res "A5 overlay scale: $(grep -m1 ' scale ' "$NESTED_DIR/overlay.log" | sed 's/^ *[0-9]* //'), fractional preferred_scale: $(grep -oE 'wp_fractional_scale_v1#[0-9]+\.preferred_scale\([0-9]+\)' "$NESTED_DIR/overlay.log" | head -1)"
    res "A5b overlay buffer/viewport: $(grep -oE 'wp_viewport#[0-9]+\.set_destination\([0-9, ]+\)' "$NESTED_DIR/overlay.log" | tail -1) $(grep -oE 'wl_shm_pool#[0-9]+\.create_buffer\([^)]*\)|zwp_linux_buffer_params_v1#[0-9]+\.create_immed\([^)]*\)' "$NESTED_DIR/overlay.log" | tail -1)"
    nested_shot "$OUT/A-overlay.png"
    # Klick mitten ins Overlay (unten mittig, 640x120, Rand 80): logisch 1600x900 -> x=800-200, y=900-80-60
    local na nb no
    na=$(count targetA '^ *[0-9]+ click') no=$(count overlay '^ *[0-9]+ click')
    nclick 600 760
    sleep 0.5
    res "A6 click on overlay area: targetA clicks +$(($(count targetA '^ *[0-9]+ click') - na)), overlay clicks +$(($(count overlay '^ *[0-9]+ click') - no)), wire pointer.enter on overlay=$(count overlay 'wl_pointer#[0-9]+\.enter')"
    res "A6b focus after click: focused=$(focused)"
    nkeys text:abc
    nflush
    sleep 0.4
    res "A7 typing with overlay mapped: targetA last text: $(grep ' text ' "$NESTED_DIR/targetA.log" | tail -1 | sed 's/^ *[0-9]* //'), overlay key lines=$(count overlay 'key press')"

    # Panel: langlebiger Prozess legt Fenster ohne Aktivierungs-Token an.
    nmsg action focus-window --id "$A"
    settle
    show_panel
    res "A8 panel shown (new window, no token): floating=$(floating dictate-panel) focused=$(focused)"
    res "A8b panel scale: $(grep -m1 ' scale ' "$NESTED_DIR/panel.log" | sed 's/^ *[0-9]* //'); xdg_activation requests: $(grep -cE -- '-> xdg_activation' "$NESTED_DIR/panel.log")"
    nmsg -j windows >"$OUT/A-windows-panel.json"
    nkeys text:hallo
    nflush
    sleep 0.4
    res "A9 typing into panel: $(grep -E '^ *[0-9]+ text ' "$NESTED_DIR/panel.log" | tail -1 | sed 's/^ *[0-9]* //')"
    nested_shot "$OUT/A-panel.png"
    nmsg action open-overview
    sleep 1.5
    nested_shot "$OUT/A-overview.png"
    nmsg action close-overview
    settle
    # MRU-Umschalter: Alt+Tab (verschachtelt Mod = Alt) halten, mehrere Bilder, dann loslassen.
    nkeys down:alt tab sleep:200 f24
    sleep 0.8
    nested_shot "$OUT/A-mru1.png"
    nkeys f24
    sleep 1.5
    nested_shot "$OUT/A-mru2.png"
    nkeys up:alt f24 sleep:50 f24
    settle
    res "A10 after Alt+Tab from panel: focused=$(focused)"
    nmsg action focus-window --id "$(win_id dictate-panel)"
    settle
    hide_panel close
    res "A11 panel destroyed (A was focused before the panel): focused=$(focused)"

    nmsg action focus-window --id "$A"
    settle
    show_panel
    res "A12 panel shown again (new window): focused=$(focused)"
    hide_panel hide
    res "A13 panel hidden (unmap): focused=$(focused)"
    show_panel
    res "A14 panel re-shown after hide: focused=$(focused) floating=$(floating dictate-panel)"
    hide_panel close
    res "A15 panel destroyed: focused=$(focused)"

    # Fokus zurück, wenn zwischendurch B fokussiert wurde
    nmsg action focus-window --id "$A"
    settle
    show_panel
    nmsg action focus-window --id "$B"
    settle
    nmsg action focus-window --id "$(win_id dictate-panel)"
    settle
    hide_panel close
    res "A16 A->panel->B->panel->close: focused=$(focused)"

    # Panel über einem Vollbildfenster
    nmsg action focus-window --id "$A"
    nmsg action fullscreen-window
    settle
    nmsg -j windows >"$OUT/A-windows-fullscreen.json"
    show_panel
    nested_shot "$OUT/A-fullscreen-panel.png"
    res "A17 panel over fullscreen A: focused=$(focused)"
    hide_panel close
    res "A17b after close: focused=$(focused)"
    nmsg action focus-window --id "$A"
    nmsg action fullscreen-window
    exec {fa}>&- {fb}>&- {fo}>&- {fp}>&-
    sleep 0.2
}

# ------------------------------------------------------------------ Teil B: Shader
partB() { # NAME ANIMDATEI
    NESTED_MODE=2600x1462 NESTED_ANIM="$2" NESTED_EXTRA_KDL="$PANEL_RULE" nested_up || return 1
    local fp fo fa
    mkfifo "$NESTED_DIR"/{p,o,a}.in
    exec {fp}<>"$NESTED_DIR/p.in" {fo}<>"$NESTED_DIR/o.in" {fa}<>"$NESTED_DIR/a.in"
    nested_spawn targetA "$PROBE" target A <"$NESTED_DIR/a.in"
    wait_line targetA mapped || return 1
    sleep 3.3
    nested_spawn panel "$PROBE" panel <"$NESTED_DIR/p.in"
    wait_line panel ready || return 1
    echo show >&"$fp"
    wait_line panel 'mapped' || return 1
    sleep 0.5
    nested_shot "$OUT/B$1-open.png"
    sleep 3.0
    echo size 1100 700 >&"$fp"
    sleep 0.5
    nested_shot "$OUT/B$1-resize.png"
    sleep 3.0
    echo close >&"$fp"
    sleep 0.5
    nested_shot "$OUT/B$1-close.png"
    sleep 3.0
    nested_spawn overlay "$PROBE" overlay <"$NESTED_DIR/o.in"
    wait_line overlay 'region empty' || return 1
    sleep 0.3
    nested_shot "$OUT/B$1-overlay.png"
    sleep 1.5
    nested_shot "$OUT/B$1-overlay2.png"
    if [[ $1 == test ]]; then
        res "B1 window-open shader (magenta px): $(pixels "$OUT/Btest-open.png" '#ff00ff')"
        res "B2 window-resize shader (yellow px): $(pixels "$OUT/Btest-resize.png" '#ffff00')"
        res "B3 window-close shader (cyan px): $(pixels "$OUT/Btest-close.png" '#00ffff')"
        res "B4 layer overlay open (magenta px at +0.3 s / +1.8 s): $(pixels "$OUT/Btest-overlay.png" '#ff00ff') / $(pixels "$OUT/Btest-overlay2.png" '#ff00ff'); teal overlay px at +1.8 s: $(pixels "$OUT/Btest-overlay2.png" '#79b8cc')"
    else
        res "B5 Daniels animations.kdl: shader warnings in niri.log: $(grep -ciE 'shader|glsl' "$NESTED_DIR/niri.log")"
        grep -iE 'shader|glsl' "$NESTED_DIR/niri.log" | head -5 | tee -a "$OUT/results.txt"
    fi
    exec {fp}>&- {fo}>&- {fa}>&-
}

# ------------------------------------------------------------------ Teil C: Variante C (Tastatur beim Kürzel greifen)
partC() {
    local grab_bind
    nested_up_c() {
        NESTED_EXTRA_KDL="binds {
    Mod+E repeat=false { spawn \"sh\" \"-c\" \"echo keyboard exclusive > $1\"; }
}" nested_up
    }
    # Pfad der FIFO muss vor nested_up feststehen: eigener Ordner unter wl-run.
    local cdir
    cdir="$(mktemp -d "$WL_RUN_BASE/c.XXXXXX")"
    mkfifo "$cdir/o.in"
    CDIR="$cdir"
    NESTED_MODE=2600x1462 nested_up_c "$cdir/o.in" || return 1
    local fo fa
    mkfifo "$NESTED_DIR/a.in"
    exec {fo}<>"$cdir/o.in" {fa}<>"$NESTED_DIR/a.in"
    nested_spawn targetA "$PROBE" target A <"$NESTED_DIR/a.in"
    wait_line targetA mapped || return 1
    WAYLAND_DEBUG=client nested_spawn overlay "$PROBE" overlay --keyboard none <"$cdir/o.in"
    wait_line overlay 'region empty' || return 1
    sleep 0.3
    # Mod+E halten (Mod = Alt verschachtelt), 800 ms, E los, dann Alt los.
    nkeys down:alt down:e sleep:800 f24 up:e sleep:300 f24 up:alt sleep:100 f24 sleep:50 f24
    sleep 0.5
    res "C1 overlay got keyboard: $(count overlay 'focus in|active true') focus lines; enter: $(grep -oE 'wl_keyboard#[0-9]+\.enter\([^)]*\)' "$NESTED_DIR/overlay.log" | head -1)"
    res "C2 overlay key events: $(grep -E 'key (press|release)|modifiers' "$NESTED_DIR/overlay.log" | sed 's/^ *//' | tr '\n' ';')"
    res "C3 wire keys on overlay: $(grep -oE 'wl_keyboard#[0-9]+\.(key|modifiers)\([^)]*\)' "$NESTED_DIR/overlay.log" | tr '\n' ';')"
    res "C4 targetA got: $(grep -E 'key (press|release)|focus' "$NESTED_DIR/targetA.log" | sed 's/^ *//' | tail -6 | tr '\n' ';')"
    exec {fo}>&- {fa}>&-
}

CDIR=
cleanup() { [[ -n ${NESTED_DIR:-} && -d ${NESTED_DIR:-/nonexistent} ]] && { keep_logs "$1"; nested_down; }; [[ -n $CDIR ]] && rm -rf "$CDIR"; CDIR=; }
trap 'cleanup abort-' EXIT
partA; cleanup A-
cat >"$OUT/anim-test.kdl" <<'EOF'
animations {
    window-open {
        duration-ms 3000
        curve "linear"
        custom-shader r#"
            vec4 open_color(vec3 coords_geo, vec3 size_geo) {
                return vec4(1.0, 0.0, 1.0, 1.0);
            }
        "#
    }
    window-close {
        duration-ms 3000
        curve "linear"
        custom-shader r#"
            vec4 close_color(vec3 coords_geo, vec3 size_geo) {
                return vec4(0.0, 1.0, 1.0, 1.0);
            }
        "#
    }
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
partB test "$OUT/anim-test.kdl"; cleanup Btest-
cp ~/.config/niri/animations.kdl "$OUT/anim-daniel.kdl" # nur gelesen
partB daniel "$OUT/anim-daniel.kdl"; cleanup Bdaniel-
partC; cleanup C-
echo "out: $OUT"
