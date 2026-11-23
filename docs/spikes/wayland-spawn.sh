#!/usr/bin/env bash
# Copyright (C) 2026 Daniel Kurdoghlian
# Pushing Pixels, Vienna <https://pushingpixels.at>
# SPDX-License-Identifier: GPL-3.0-or-later
# Spike E10 (W5 Block A, PLAN-wayland.md 6g): Wo landet ein Prozess, den ein Aufrufer über niri-IPC `Action::Spawn`
# startet? Läuft nur im verschachtelten niri (tests/wayland/nested/harness.sh), nie gegen die echte Sitzung.
#
# Ansatz 1: der Aufrufer (`niri msg action spawn`) läuft in einer eigenen transienten Unit des User-Managers mit
# KillMode=control-group (wie dictate-ui.service). Gemessen wird Cgroup, Eltern, Umgebung und Arbeitsordner des
# gestarteten Prozesses; danach wird die Unit gestoppt und geprüft, ob der Prozess überlebt.
# Dazu: die Liste der Arbeitsflächen (steht die leere am Ende darin?) und die Antwort bei einem fehlenden Programm.
#
# Nutzung: docs/spikes/wayland-spawn.sh [AUSGABEORDNER]   (Vorgabe /home/data/stt-test/logs/w5a/spike)
# Braucht den Injektor des Harness (NESTED_INJECT oder tests/wayland/nested/target/debug/nested-inject).
set -u
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
OUT="${1:-/home/data/stt-test/logs/w5a/spike}"
mkdir -p "$OUT"
# shellcheck source=../../tests/wayland/nested/harness.sh
source "$HERE/tests/wayland/nested/harness.sh"

UID_RT=/run/user/$(id -u)
user_systemd() { # der User-Manager über den echten Bus (nur für die eigene transiente Unit)
    env XDG_RUNTIME_DIR="$UID_RT" DBUS_SESSION_BUS_ADDRESS="unix:path=$UID_RT/bus" "$@"
}
UNIT="dictate-w5-spike-caller-$$"
SPAWNED=()

cleanup() {
    local p
    user_systemd /usr/bin/systemctl --user stop "$UNIT" >/dev/null 2>&1
    for p in "${SPAWNED[@]}"; do kill -TERM "$p" 2>/dev/null; done
    [[ -n ${NESTED_DIR:-} && -f $NESTED_DIR/niri.log ]] && cp -f "$NESTED_DIR/niri.log" "$OUT/niri.log"
    nested_down
    if [[ -n ${NESTED_RT:-} ]]; then
        for p in $(pgrep -u "$(id -u)"); do
            tr '\0' '\n' <"/proc/$p/environ" 2>/dev/null | grep -qx "XDG_RUNTIME_DIR=$NESTED_RT" &&
                echo "LEFT OVER: $p $(tr '\0' ' ' <"/proc/$p/cmdline" | cut -c1-80)" | tee -a "$OUT/result.txt" >&2
        done
    fi
    [[ -n ${NESTED_DIR:-} && $NESTED_DIR == "$WL_RUN_BASE"/n.* ]] && rm -rf "$NESTED_DIR"
}
trap cleanup EXIT

# PID des `sleep` mit genau diesem Argument (eindeutige Dauer je Lauf), nur unter den eigenen Prozessen
find_sleep() { # ARG
    local p
    for p in $(pgrep -u "$(id -u)" -x sleep); do
        [[ "$(tr '\0' ' ' <"/proc/$p/cmdline" 2>/dev/null)" == "/usr/bin/sleep $1 " ]] && { echo "$p"; return 0; }
    done
    return 1
}

report() { echo "$*" | tee -a "$OUT/result.txt"; }
: >"$OUT/result.txt"

nested_up || { echo "harness did not come up" >&2; exit 1; }
case "$NESTED_NIRI_SOCKET" in
"$NESTED_RT"/*) ;;
*) echo "refusing: niri socket outside the private runtime dir" >&2; exit 70 ;;
esac
cp "$NESTED_CONFIG_FILE" "$OUT/niri-config.kdl"

report "niri pid $NESTED_NIRI_PID cgroup $(cat /proc/"$NESTED_NIRI_PID"/cgroup)"
report "niri cwd $(readlink /proc/"$NESTED_NIRI_PID"/cwd)"
report "harness shell cgroup $(cat /proc/$$/cgroup)"

# Arbeitsflächen: zwei Fenster öffnen, dann die Liste (die leere am Ende?)
# Fenster: alacritty mit festem Befehl (`-e`), nie die Login-Shell: die startete auf blub zellij-Server, die sich
# ablösen und den Lauf überleben (erster Lauf am 21.11.)
nmsg -j workspaces >"$OUT/workspaces-empty.json"
for n in 1 2; do
    W="3700.$n$$"
    nmsg action focus-workspace "$n" >/dev/null 2>&1
    nmsg action spawn -- /usr/bin/alacritty -e /usr/bin/sleep "$W" >/dev/null 2>&1 || true
    nested_wait 5 find_sleep "$W" && SPAWNED+=("$(find_sleep "$W")")
    sleep 1
done
nmsg -j workspaces >"$OUT/workspaces-two.json"
nmsg -j windows >"$OUT/windows.json"
for p in $(pgrep -u "$(id -u)" -x alacritty); do
    tr '\0' '\n' <"/proc/$p/environ" 2>/dev/null | grep -qx "XDG_RUNTIME_DIR=$NESTED_RT" && SPAWNED+=("$p")
done
report "workspaces (no windows): $(python3 -c 'import json,sys; print([(w["idx"], w["output"], w["is_active"]) for w in json.load(open(sys.argv[1]))])' "$OUT/workspaces-empty.json")"
report "workspaces (windows on 1 and 2): $(python3 -c 'import json,sys; print(sorted((w["idx"], w["output"], w["active_window_id"] is not None) for w in json.load(open(sys.argv[1]))))' "$OUT/workspaces-two.json")"

# Ansatz 1: Aufrufer in eigener Unit (KillMode=control-group wie dictate-ui.service)
ARG="3600.$$"
user_systemd /usr/bin/systemd-run --user --quiet --collect --unit="$UNIT" -p KillMode=control-group \
    -E NIRI_SOCKET="$NESTED_NIRI_SOCKET" -E SPIKE_MARKER="caller-$$" -E PATH=/usr/bin \
    /usr/bin/bash -c "/usr/bin/niri msg action spawn -- /usr/bin/sleep $ARG >\"$OUT/caller-spawn.txt\" 2>&1; echo exit=\$? >>\"$OUT/caller-spawn.txt\"; exec /usr/bin/sleep 300" ||
    { echo "systemd-run failed" >&2; exit 1; }
nested_wait 5 find_sleep "$ARG" || { echo "spawned process not found" >&2; cat "$OUT/caller-spawn.txt" >&2; exit 1; }
S="$(find_sleep "$ARG")"
SPAWNED+=("$S")
UNIT_CG="$(user_systemd /usr/bin/systemctl --user show -p ControlGroup --value "$UNIT")"
report "caller unit $UNIT cgroup $UNIT_CG"
report "caller answer: $(tr '\n' ' ' <"$OUT/caller-spawn.txt")"
report "spawned pid $S ppid $(awk '{print $4}' /proc/"$S"/stat) (niri $NESTED_NIRI_PID)"
report "spawned cgroup $(cat /proc/"$S"/cgroup)"
report "spawned cwd $(readlink /proc/"$S"/cwd)"
report "spawned has SPIKE_MARKER of the caller: $(tr '\0' '\n' </proc/"$S"/environ | grep -c '^SPIKE_MARKER=')"
report "spawned XDG_RUNTIME_DIR: $(tr '\0' '\n' </proc/"$S"/environ | grep '^XDG_RUNTIME_DIR=')"
report "spawned WAYLAND_DISPLAY: $(tr '\0' '\n' </proc/"$S"/environ | grep '^WAYLAND_DISPLAY=')"
user_systemd /usr/bin/systemctl --user stop "$UNIT"
sleep 0.5
if kill -0 "$S" 2>/dev/null; then report "after stopping the caller unit: spawned process alive"; else report "after stopping the caller unit: spawned process GONE"; fi

# Arbeitsordner über /usr/bin/env -C (Path= eines Desktop-Eintrags)
ARG2="3601.$$"
nmsg action spawn -- /usr/bin/env -C /usr/share -- /usr/bin/sleep "$ARG2" >/dev/null
nested_wait 5 find_sleep "$ARG2" && { S2="$(find_sleep "$ARG2")"; SPAWNED+=("$S2"); report "env -C: cwd $(readlink /proc/"$S2"/cwd)"; }

# Fehlendes Programm: was antwortet niri?
MISSING="/nonexistent/dictate-spike-$$"
ANSWER="$(nmsg action spawn -- "$MISSING" 2>&1)"
report "missing program: exit $? answer '${ANSWER}'"
sleep 0.5
report "niri log about it: $(grep -c "dictate-spike-$$" "$NESTED_DIR/niri.log") line(s): $(grep "dictate-spike-$$" "$NESTED_DIR/niri.log" | head -n 2 | tr '\n' ' ')"
report "niri log about scopes: $(grep -i -E 'scope|systemd' "$NESTED_DIR/niri.log" | head -n 3 | tr '\n' ' ')"
