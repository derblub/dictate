#!/usr/bin/env bash
# Copyright (C) 2026 Daniel Kurdoghlian
# Pushing Pixels, Vienna <https://pushingpixels.at>
# SPDX-License-Identifier: GPL-3.0-or-later
# Spike S2, Browser: T2 (virtuelle Tastatur) und T3 (Eingabemethode) in Firefox und Chromium (ohne und mit
# --enable-wayland-ime --wayland-text-input-version=3). Chromium steht für Electron (Slack, Orca).
# Wegwerf-Profile im privaten Laufzeitordner, nie die echten Profile. Ergebnis über den Fenstertitel
# (die Seite schreibt den Inhalt des Textfelds in document.title).
# Aufruf: bash spikes/wayland-vkbd/s2b.sh [AUSGABEORDNER]
set -uo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$HERE/../../tests/wayland/nested/harness.sh"
OUT="${1:-/home/data/stt-test/logs/wl-spikes/s2b/$(date +%H%M%S)}"
mkdir -p "$OUT"
VK="$HERE/target/debug/vk"
: >"$OUT/results.txt"
res() { echo "$*" | tee -a "$OUT/results.txt"; }
TEXT="${S2_TEXT:-Grüße, Maß: äöü ÄÖÜ ß € – „ok“}"
cleanup() {
    if [[ -n ${NESTED_DIR:-} && -d ${NESTED_DIR:-/nonexistent} ]]; then
        local f
        for f in "$NESTED_DIR"/*.log; do cp "$f" "$OUT/$(basename "$f")"; done
        nested_down
    fi
}
trap cleanup EXIT

win_by_app() { nmsg -j windows | python3 -c 'import json,sys; t=sys.argv[1]; print(next((str(w["id"]) for w in json.load(sys.stdin) if t in (w["app_id"] or "").lower()), ""))' "$1"; }
title_of() { nmsg -j windows | python3 -c 'import json,sys; i=int(sys.argv[1]); print(next((w["title"] for w in json.load(sys.stdin) if w["id"]==i), "?"))' "$1"; }
wait_app() { local d=$((SECONDS + ${2:-60})); until [[ -n $(win_by_app "$1") ]]; do ((SECONDS >= d)) && return 1; sleep 0.2; done; }
wait_title() { local d=$((SECONDS + 30)); until [[ $(title_of "$1") == *"$2"* ]]; do ((SECONDS >= d)) && return 1; sleep 0.2; done; }

declare -A VKFD
vk_start() {
    mkfifo "$NESTED_DIR/$1.in"
    local fd
    exec {fd}<>"$NESTED_DIR/$1.in"
    VKFD[$1]=$fd
    nested_spawn "$1" env ${VK_ENV:-} "$VK" "$2" <"$NESTED_DIR/$1.in"
    local d=$((SECONDS + 10))
    until grep -qE ' (ready|error)' "$NESTED_DIR/$1.log" 2>/dev/null; do ((SECONDS >= d)) && return 1; sleep 0.05; done
    sleep 0.4
}
vk() {
    local n
    n=$(grep -cE ' (ok|error) ' "$NESTED_DIR/$1.log")
    echo "${*:2}" >&"${VKFD[$1]}"
    local d=$((SECONDS + 15))
    until (($(grep -cE ' (ok|error) ' "$NESTED_DIR/$1.log") > n)); do ((SECONDS >= d)) && return 1; sleep 0.02; done
}

NESTED_MODE=1920x1080 NESTED_SCALE_CFG=1 nested_up || exit 1
cat >"$NESTED_DIR/tmp/page.html" <<'EOF'
<!doctype html><meta charset="utf-8"><title>READY</title>
<textarea id="t" autofocus style="width:90vw;height:60vh;font-size:24px"
 oninput="document.title='T:'+JSON.stringify(this.value)"></textarea>
<script>document.getElementById('t').focus()</script>
EOF
PAGE="file://$NESTED_DIR/tmp/page.html"

browser_case() { # NAME APP_ID_TEIL CMD...
    local name=$1 app=$2
    shift 2
    nested_spawn "$name" "$@"
    wait_app "$app" 90 || { res "$name: no window"; return 1; }
    local id
    id=$(win_by_app "$app")
    wait_title "$id" READY || res "$name: page title not READY ($(title_of "$id"))"
    nmsg action focus-window --id "$id"
    sleep 1.5
    vk_start "k$name" kbd
    vk "k$name" "type $TEXT"
    sleep 1.5
    res "T2-$name (keymap grows while typing): $(title_of "$id")"
    vk "k$name" "type \n"
    vk_start "p$name" kbd
    vk "p$name" "prepare $TEXT"
    sleep 0.2
    vk "p$name" "type $TEXT"
    sleep 1.5
    res "T2-$name (keymap prepared once): $(title_of "$id")"
    vk "p$name" "type \n"
    vk_start "i$name" im
    sleep 1.5
    res "T3-$name IM events: $(grep -E ' (activate|deactivate|content_type|unavailable)' "$NESTED_DIR/i$name.log" | sed 's/^ *[0-9]* //' | tr '\n' ';')"
    vk "i$name" "commit $TEXT"
    sleep 1.5
    res "T3-$name after commit: $(title_of "$id") [$(grep ' committed' "$NESTED_DIR/i$name.log" | tail -1 | sed 's/^ *[0-9]* //')]"
    vk "i$name" destroy
    echo "" >&"${VKFD[k$name]}"
    nmsg -j windows >"$OUT/windows-$name.json"
    nmsg action close-window --id "$id"
    local d=$((SECONDS + 20))
    while [[ -n $(win_by_app "$app") ]] && ((SECONDS < d)); do sleep 0.3; done
    res "$name closed: $([[ -z $(win_by_app "$app") ]] && echo yes || echo NO)"
}

# Firefox: Wegwerfprofil, kein Netz-Ballast
FFP="$NESTED_DIR/tmp/ff"
mkdir -p "$FFP"
cat >"$FFP/user.js" <<'EOF'
user_pref("browser.shell.checkDefaultBrowser", false);
user_pref("browser.aboutwelcome.enabled", false);
user_pref("trailhead.firstrun.didSeeAboutWelcome", true);
user_pref("browser.startup.homepage_override.mstone", "ignore");
user_pref("datareporting.policy.dataSubmissionPolicyBypassNotification", true);
user_pref("datareporting.healthreport.uploadEnabled", false);
user_pref("toolkit.telemetry.reportingpolicy.firstRun", false);
user_pref("toolkit.telemetry.enabled", false);
user_pref("app.update.enabled", false);
user_pref("extensions.update.enabled", false);
user_pref("browser.safebrowsing.malware.enabled", false);
user_pref("browser.safebrowsing.phishing.enabled", false);
user_pref("network.connectivity-service.enabled", false);
user_pref("network.captive-portal-service.enabled", false);
user_pref("browser.newtabpage.enabled", false);
EOF
FIREFOX_PID_NOTE=1
if [[ -n ${ONLY_DIAG:-} ]]; then
    diag() {
        local id
        nested_spawn chromium chromium --user-data-dir="$NESTED_DIR/tmp/chr0" "${CHR_COMMON[@]}" --app="$PAGE"
        wait_app chrom 90 || return 1
        id=$(win_by_app chrom)
        wait_title "$id" READY
        nmsg action focus-window --id "$id"
        sleep 1.5
        local seg n=0 pre
        # Hypothese: Chromium deutet evdev-Code 14 (KEY_BACKSPACE) trotz anderem Keysym als Rücktaste.
        # Vorbelegung "bcdefghij" + Zeichen X: Mods 1-4, b..j 5-13, X = Code 14. Getippt wird "bcX|".
        for seg in ":" "," "q" "ß" "-"; do
            n=$((n + 1))
            vk_start "kd$n" kbd
            vk "kd$n" "prepare bcdefghij$seg|"
            sleep 0.1
            vk "kd$n" "type bc$seg|"
            sleep 0.5
        done
        sleep 1
        res "DIAG chromium segments: $(title_of "$id")"
        nmsg action close-window --id "$id"
        sleep 2
    }
    CHR_COMMON=(--ozone-platform=wayland --no-first-run --no-default-browser-check --disable-background-networking
        --disable-component-update --disable-sync --password-store=basic --disable-gpu)
    diag
    exit 0
fi
[[ -n ${SKIP_FF:-} ]] || MOZ_ENABLE_WAYLAND=1 browser_case firefox firefox firefox --no-remote --new-instance --profile "$FFP" "$PAGE"
CHR_COMMON=(--ozone-platform=wayland --no-first-run --no-default-browser-check --disable-background-networking
    --disable-component-update --disable-sync --password-store=basic --disable-gpu)
browser_case chromium chrom chromium --user-data-dir="$NESTED_DIR/tmp/chr1" "${CHR_COMMON[@]}" --app="$PAGE"
browser_case chromium-ime chrom chromium --user-data-dir="$NESTED_DIR/tmp/chr2" "${CHR_COMMON[@]}" \
    --enable-wayland-ime --wayland-text-input-version=3 --app="$PAGE"
res "done"
