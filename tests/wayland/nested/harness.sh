#!/usr/bin/env bash
# Copyright (C) 2026 Daniel Kurdoghlian
# Pushing Pixels, Vienna <https://pushingpixels.at>
# SPDX-License-Identifier: GPL-3.0-or-later
# Verschachteltes niri in kopflosem sway für den Prüfschritt `wayland` (tests/wayland/run.sh) und die Spikes aus W0.
# Muster: niri-coach tests/nested/harness.sh.
#
# Nutzung als Bibliothek (immer in EINEM bash-Skript: hoch, testen, runter; trap auf EXIT):
#   source tests/wayland/nested/harness.sh; nested_up; nmsg -j outputs; nested_down
#
# Isolation (hart):
# - DISPLAY, WAYLAND_DISPLAY, NIRI_SOCKET, SWAYSOCK, DBUS_SESSION_BUS_ADDRESS, DBUS_SYSTEM_BUS_ADDRESS und
#   XDG_SESSION_ID werden entfernt.
# - Eigener System-Bus: ein privater dbus-daemon (ohne Aktivierung von Diensten) im Laufzeitordner, exportiert als
#   DBUS_SYSTEM_BUS_ADDRESS, bevor sway und niri starten. Alles, was gio als System-Bus öffnet (die Sperr-Erkennung von
#   dictate-ui über logind), landet dort; ein falsches logind belegt den Namen (tests/wayland/run.sh). Das echte logind
#   bleibt unberührt.
# - Eigener kurzer Laufzeitordner unter $DICTATE_TEST_ROOT/wl-run (scripts/test-root.sh, Vorgabe
#   ~/.cache/dictate-test/wl-run; Unix-Sockets über 107 Zeichen scheitern, darum prüft nested_root_fits die Länge),
#   0700, als XDG_RUNTIME_DIR und für die übrigen XDG_*-Ordner. Injektor und Zwischenablage (nested/src/guard.rs)
#   verweigern den Start außerhalb davon. Die Spike-Programme unter spikes/ prüfen noch den festen Ordner
#   /home/data/stt-test/wl-run und laufen darum nur mit DICTATE_TEST_ROOT=/home/data/stt-test (Archiv).
# - DICTATE_TEST_ROOT wird beim Einbinden aufgelöst und exportiert, bevor nested_up XDG_CACHE_HOME umbiegt; ist der
#   Ordner untauglich, bleibt WL_RUN_BASE leer und nested_up scheitert.
# - `nmsg` lehnt jeden niri-Socket außerhalb des eigenen Laufzeitordners ab: nie ein `niri msg` an das echte niri.
# - Beendet nur die eigenen PIDs bzw. Prozessgruppen, nie per Name.
#
# Stellschrauben (Umgebung, gelesen von nested_up):
#   NESTED_MODE      sway-Ausgabe, Vorgabe 5120x2160 (wie DP-4; niri-Skalierung 1,625 -> 3150x1329 logisch)
#   NESTED_SCALE_CFG niri-Skalierung der Ausgabe, Vorgabe 1.625
#   NESTED_LAYOUT    xkb-Belegung des verschachtelten niri, Vorgabe "de" / NESTED_VARIANT "nodeadkeys" (wie blub)
#   NESTED_ANIM      Datei mit einem animations-Block; ohne: animations { off }
#   NESTED_EXTRA_KDL zusätzliches KDL (Fensterregeln, Kürzel, xwayland-satellite)
#   NESTED_XWAYLAND  1 = xwayland-satellite im verschachtelten niri an (Vorgabe aus)

NESTED_HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# Längster Socketname unter dem Laufzeitordner: sway-ipc.<uid>.<pid>.sock (uid bis 10, pid bis 7 Ziffern); kürzer
# sind niri.<wayland-N>.<pid>.sock, dictate/control.sock, dictate/ui.sock und system_bus_socket.
NESTED_SOCK_NAME_MAX=32
# Laufzeitordner wird $WL_RUN_BASE/n.XXXXXX/rt (mktemp); ein Socket darin hat höchstens 107 Zeichen (sun_path 108 mit
# abschließender Null).
nested_root_fits() {
    local tail="/wl-run/n.XXXXXX/rt/"
    local max_root=$((107 - NESTED_SOCK_NAME_MAX - ${#tail}))
    ((${#DICTATE_TEST_ROOT} <= max_root)) || {
        echo "DICTATE_TEST_ROOT '$DICTATE_TEST_ROOT' is too long for Unix socket paths of the nested session" \
            "(at most $max_root characters); set DICTATE_TEST_ROOT to a shorter folder." >&2
        return 1
    }
}
# shellcheck source=../../../scripts/test-root.sh
source "$NESTED_HERE/../../../scripts/test-root.sh"
if dictate_test_root && nested_root_fits; then
    WL_RUN_BASE="$DICTATE_TEST_ROOT/wl-run"
else
    WL_RUN_BASE=""
fi
NESTED_PIDS=()
NESTED_PGIDS=()

nested_log() { echo "nested: $*" >&2; }

nested_wait() { # SEKUNDEN CMD...: CMD alle 50 ms, bis es gelingt
    local deadline=$((SECONDS + $1))
    shift
    until "$@" >/dev/null 2>&1; do
        ((SECONDS >= deadline)) && return 1
        sleep 0.05
    done
}

nmsg() { # niri msg nur gegen den verschachtelten Socket
    [[ -n ${NESTED_RT:-} && -n ${NESTED_NIRI_SOCKET:-} ]] || { nested_log "nmsg: harness not up"; return 70; }
    case "$NESTED_NIRI_SOCKET" in
    "$NESTED_RT"/*) ;;
    *) nested_log "nmsg: refusing socket outside the private runtime dir"; return 70 ;;
    esac
    NIRI_SOCKET="$NESTED_NIRI_SOCKET" niri msg "$@"
}

nswaymsg() { SWAYSOCK="$NESTED_SWAYSOCK" swaymsg "$@"; }

# Client im verschachtelten niri starten, eigene Prozessgruppe (setsid), Log unter NESTED_DIR/<name>.log.
# Gibt die PID in NESTED_LAST_PID zurück. Stdin bleibt die des Aufrufers (`nested_spawn x cmd <fifo`).
nested_spawn() { # NAME CMD...
    local log="$NESTED_DIR/$1.log"
    shift
    WAYLAND_DISPLAY="$NESTED_NIRI_WL" setsid "$@" <&0 >"$log" 2>&1 &
    NESTED_LAST_PID=$!
    NESTED_PIDS+=($!)
    NESTED_PGIDS+=($!)
}

# Physische Tastatur und Maus des verschachtelten niri: virtuelles Gerät auf dem kopflosen sway (wie niri-coach
# inject_input). niri sieht diese Tasten wie eine echte Tastatur (winit-Backend).
nested_input_start() {
    [[ -x ${NESTED_INJECT:-} ]] || { nested_log "injector $NESTED_INJECT not built"; return 1; }
    mkfifo "$NESTED_DIR/inj.in" "$NESTED_DIR/inj.out" || return 1
    WAYLAND_DISPLAY="$NESTED_SWAY_WL" "$NESTED_INJECT" --keymap "$NESTED_DIR/host.xkb" --extent "$NESTED_EXTENT" \
        <"$NESTED_DIR/inj.in" >"$NESTED_DIR/inj.out" 2>"$NESTED_DIR/inj.log" &
    NESTED_PIDS+=($!)
    exec {NESTED_INJ_IN}>"$NESTED_DIR/inj.in" {NESTED_INJ_OUT}<"$NESTED_DIR/inj.out"
    local cx=$((${NESTED_EXTENT%,*} / 2)) cy=$((${NESTED_EXTENT#*,} / 2))
    nkeys "move:$cx,$cy" sleep:100 "move:$((cx + 1)),$cy" sleep:100
}

nkeys() { # Schritte an den Injektor (siehe tests/wayland/nested/src/main.rs)
    [[ -n ${NESTED_INJ_IN:-} ]] || { nested_log "nkeys: injector not running"; return 1; }
    local reply
    echo "$*" >&"$NESTED_INJ_IN"
    read -r -t 10 reply <&"$NESTED_INJ_OUT" || reply="error: no reply"
    [[ $reply == ok ]] || { nested_log "nkeys $*: $reply"; return 1; }
}

# Klick auf logische niri-Koordinaten. Verschachteltes niri reicht ein Host-Ereignis erst mit dem nächsten weiter
# (niri-coach sp-1.md); neutrales F24 schiebt nach, ohne Modifikatoren zu ändern.
nclick() {
    local px py
    px=$(python3 -c "print(round($1 * $NESTED_SCALE))") py=$(python3 -c "print(round($2 * $NESTED_SCALE))")
    nkeys "move:$px,$py" sleep:50 f24 sleep:50 press sleep:50 f24 sleep:50 release sleep:50 f24 sleep:50 f24
}
nflush() { nkeys sleep:50 f24 sleep:50 f24; }

nested_config() { # schreibt die Test-Config nach $1
    local layout="${NESTED_LAYOUT:-de}" variant="${NESTED_VARIANT-nodeadkeys}" scale="${NESTED_SCALE_CFG:-1.625}"
    cat <<EOF
// Test-Config, erzeugt von tests/wayland/nested/harness.sh. Verschachtelt in winit gilt Mod = Alt.
input {
    keyboard {
        xkb {
            layout "$layout"
            variant "$variant"
        }
        repeat-delay 275
        repeat-rate 35
    }
}
output "winit" {
    scale $scale
}
layout {
    gaps 12
    default-column-width { proportion 0.5; }
    focus-ring {
        width 2
        active-color "#79b8cc"
        inactive-color "#464442"
    }
}
gestures {
    hot-corners {
        off
    }
}
prefer-no-csd
hotkey-overlay {
    skip-at-startup
}
screenshot-path null
EOF
    if [[ -n ${NESTED_ANIM:-} ]]; then cat "$NESTED_ANIM"; else printf 'animations {\n    off\n}\n'; fi
    [[ ${NESTED_XWAYLAND:-0} == 1 ]] || printf 'xwayland-satellite {\n    off\n}\n'
    printf '%s\n' "${NESTED_EXTRA_KDL:-}"
}

# Privater Bus als System-Bus (vor sway und niri, damit alle Programme ihn erben). Eigene Config: keine
# Aktivierung von Diensten (die Session-Config des Systems startete sonst echte Dienste an diesem Bus).
nested_system_bus() {
    command -v dbus-daemon >/dev/null || { nested_log "missing dbus-daemon"; return 1; }
    cat >"$NESTED_DIR/sysbus.conf" <<EOF
<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
 "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig>
  <type>session</type>
  <listen>unix:path=$NESTED_RT/system_bus_socket</listen>
  <auth>EXTERNAL</auth>
  <policy context="default">
    <allow send_destination="*" eavesdrop="true"/>
    <allow eavesdrop="true"/>
    <allow own="*"/>
  </policy>
</busconfig>
EOF
    dbus-daemon --config-file="$NESTED_DIR/sysbus.conf" --nofork --print-address=3 3>"$NESTED_DIR/sysbus.addr" \
        >/dev/null 2>"$NESTED_DIR/sysbus.log" &
    NESTED_PIDS+=($!)
    nested_wait 5 test -s "$NESTED_DIR/sysbus.addr" || { nested_log "private system bus did not start"; return 1; }
    DBUS_SYSTEM_BUS_ADDRESS="$(head -n 1 "$NESTED_DIR/sysbus.addr")"
    case "$DBUS_SYSTEM_BUS_ADDRESS" in
    "unix:path=$NESTED_RT/"*) export DBUS_SYSTEM_BUS_ADDRESS ;;
    *) nested_log "private system bus has an unexpected address"; return 1 ;;
    esac
}

nested_up() {
    local mode="${NESTED_MODE:-5120x2160}"
    command -v sway >/dev/null && command -v niri >/dev/null || { nested_log "missing sway or niri"; return 1; }
    [[ -n $WL_RUN_BASE ]] || { nested_log "no usable test folder (DICTATE_TEST_ROOT, message above)"; return 1; }
    mkdir -p "$WL_RUN_BASE" || return 1
    NESTED_DIR="$(mktemp -d "$WL_RUN_BASE/n.XXXXXX")" || return 1
    chmod 700 "$NESTED_DIR"
    NESTED_RT="$NESTED_DIR/rt"
    mkdir -p "$NESTED_RT" "$NESTED_DIR"/{config,state,cache,data,niri,tmp} || return 1
    chmod 700 "$NESTED_RT"

    unset DISPLAY WAYLAND_DISPLAY NIRI_SOCKET SWAYSOCK DBUS_SESSION_BUS_ADDRESS DBUS_SYSTEM_BUS_ADDRESS XDG_SESSION_ID
    export XDG_RUNTIME_DIR="$NESTED_RT" XDG_CONFIG_HOME="$NESTED_DIR/config" XDG_STATE_HOME="$NESTED_DIR/state"
    export XDG_CACHE_HOME="$NESTED_DIR/cache" XDG_DATA_HOME="$NESTED_DIR/data" TMPDIR="$NESTED_DIR/tmp"
    export GTK_USE_PORTAL=0 NO_AT_BRIDGE=1 GSETTINGS_BACKEND=memory GIO_USE_VFS=local
    nested_system_bus || return 1

    NESTED_CONFIG_FILE="$NESTED_DIR/niri/config.kdl"
    nested_config >"$NESTED_CONFIG_FILE"
    niri validate -c "$NESTED_CONFIG_FILE" >"$NESTED_DIR/validate.log" 2>&1 || {
        nested_log "niri validate failed: $(tail -n 5 "$NESTED_DIR/validate.log")"
        return 1
    }

    cat >"$NESTED_DIR/sway.conf" <<EOF
xwayland disable
default_border none
default_floating_border none
output HEADLESS-1 mode $mode scale 1
EOF
    env WLR_BACKENDS=headless WLR_LIBINPUT_NO_DEVICES=1 WLR_RENDERER=pixman \
        sway -c "$NESTED_DIR/sway.conf" >"$NESTED_DIR/sway.log" 2>&1 &
    NESTED_SWAY_PID=$!
    NESTED_PIDS+=($!)
    nested_wait 10 compgen -G "$NESTED_RT/sway-ipc.*.sock" || { nested_log "sway did not start"; return 1; }
    NESTED_SWAYSOCK="$(compgen -G "$NESTED_RT/sway-ipc.*.sock" | head -n 1)"
    NESTED_SWAY_WL="$(cd "$NESTED_RT" && compgen -G 'wayland-[0-9]' | head -n 1)"
    [[ -n $NESTED_SWAY_WL ]] || { nested_log "no sway wayland socket"; return 1; }

    env WAYLAND_DISPLAY="$NESTED_SWAY_WL" ${NESTED_NIRI_ENV:-} niri -c "$NESTED_CONFIG_FILE" >"$NESTED_DIR/niri.log" 2>&1 &
    NESTED_NIRI_PID=$!
    NESTED_PIDS+=($!)
    nested_wait 20 compgen -G "$NESTED_RT/niri.*.$NESTED_NIRI_PID.sock" || { nested_log "niri did not start"; return 1; }
    NESTED_NIRI_SOCKET="$(compgen -G "$NESTED_RT/niri.*.$NESTED_NIRI_PID.sock" | head -n 1)"
    NESTED_NIRI_WL="$(basename "$NESTED_NIRI_SOCKET")"
    NESTED_NIRI_WL="${NESTED_NIRI_WL#niri.}"
    NESTED_NIRI_WL="${NESTED_NIRI_WL%.$NESTED_NIRI_PID.sock}"
    nested_wait 10 nmsg version || { nested_log "nested niri does not answer"; return 1; }
    grep -q 'error loading config' "$NESTED_DIR/niri.log" && { nested_log "config error"; return 1; }
    export WAYLAND_DISPLAY="$NESTED_SWAY_WL" # grim und der Injektor sprechen mit dem kopflosen sway
    xkbcli compile-keymap --layout us >"$NESTED_DIR/host.xkb" || return 1
    NESTED_INJECT="${NESTED_INJECT:-$NESTED_HERE/target/debug/nested-inject}"
    NESTED_EXTENT="$(nswaymsg -t get_outputs | python3 -c 'import json,sys; r=json.load(sys.stdin)[0]["rect"]; print(r["width"], r["height"], sep=",")')"
    NESTED_SCALE="$(nmsg -j outputs | python3 -c 'import json,sys; print(next(iter(json.load(sys.stdin).values()))["logical"]["scale"])')"
    nested_input_start || return 1
    nested_log "up: dir=$NESTED_DIR niri-wl=$NESTED_NIRI_WL scale=$NESTED_SCALE mode=$mode"
}

nested_shot() { grim "$1"; }

# Alles beenden, was gestartet wurde: Prozessgruppen der Clients, dann sway/niri/Injektor; Kinder von niri
# (xwayland-satellite, gespawnte Programme) werden vorher eingesammelt.
nested_down() {
    local p alive=() kids=()
    if [[ -n ${NESTED_INJ_IN:-} ]]; then
        exec {NESTED_INJ_IN}>&- {NESTED_INJ_OUT}<&-
        unset NESTED_INJ_IN NESTED_INJ_OUT
    fi
    [[ -n ${NESTED_NIRI_PID:-} ]] && mapfile -t kids < <(pgrep -P "$NESTED_NIRI_PID" 2>/dev/null)
    for p in "${NESTED_PGIDS[@]}"; do kill -TERM -- "-$p" 2>/dev/null; done
    for p in "${kids[@]}" "${NESTED_PIDS[@]}"; do kill -TERM "$p" 2>/dev/null; done
    for _ in $(seq 60); do
        alive=()
        for p in "${kids[@]}" "${NESTED_PIDS[@]}"; do kill -0 "$p" 2>/dev/null && alive+=("$p"); done
        for p in "${NESTED_PGIDS[@]}"; do pgrep -g "$p" >/dev/null 2>&1 && alive+=("g$p"); done
        ((${#alive[@]} == 0)) && break
        sleep 0.05
    done
    for p in "${NESTED_PGIDS[@]}"; do kill -KILL -- "-$p" 2>/dev/null; done
    for p in "${kids[@]}" "${NESTED_PIDS[@]}"; do kill -KILL "$p" 2>/dev/null; done
    for p in "${NESTED_PIDS[@]}"; do wait "$p" 2>/dev/null; done
    alive=()
    for p in "${kids[@]}" "${NESTED_PIDS[@]}"; do kill -0 "$p" 2>/dev/null && alive+=("$p"); done
    NESTED_PIDS=()
    NESTED_PGIDS=()
    local leftover
    leftover="$(fuser "$NESTED_RT" 2>/dev/null | tr -s ' ')"
    [[ -n ${NESTED_KEEP:-} ]] || rm -rf -- "${NESTED_DIR:?}"
    if ((${#alive[@]})) || [[ -n $leftover ]]; then
        nested_log "teardown left processes: ${alive[*]} $leftover"
        return 1
    fi
    nested_log "down: no process left"
}
