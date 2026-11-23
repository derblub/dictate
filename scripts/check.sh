#!/usr/bin/env bash
# Copyright (C) 2026 Daniel Kurdoghlian
# Pushing Pixels, Vienna <https://pushingpixels.at>
# SPDX-License-Identifier: GPL-3.0-or-later
# Alle Prüfungen von dictate in einem Befehl, billige zuerst; Abbruch beim ersten Fehler. Einzige Prüfquelle für den
# User, für Agenten (nur die betroffenen Schritte mit --only) und für eine spätere CI.
#
# Aufruf: scripts/check.sh              alles
#         scripts/check.sh --list       Schritte zeigen
#         scripts/check.sh --only a,b   nur diese Schritte (in der Reihenfolge der Liste)
#
# Schritte:
#   fmt     cargo fmt --check (rustfmt.toml)
#   licenses cargo deny check licenses (Erlaubnisliste in deny.toml) und THIRD-PARTY-LICENSES.txt passend zu
#           Cargo.lock und Toolchain (scripts/third-party-licenses.py --check); offline, ohne Build, unter 2 s
#   clippy  cargo clippy --release --all-targets, Warnungen sind Fehler
#   unit    cargo test --release (Mock-Tests, ohne ignorierte)
#   wayland dictate-ui im verschachtelten niri (kopfloses sway, pixman) gegen einen Mock-Daemon: Overlay ohne Fokus und
#           klick-durch, Kürzel aus niri/dictate-binds.kdl, Halten/Loslassen, Rückfall Umschalten, Abbruch; Textausgabe
#           über Eingabemethode, virtuelle Tastatur und Zwischenablage in GTK, Alacritty und xterm (XWayland);
#           Textaktionen (PRIMARY, Panel, Strom, Copy, Replace, Schließen, Maus) und Sperre über ein falsches logind
#           auf einem privaten System-Bus (tests/wayland/run.sh, output.sh, actions.sh, lock.sh; Harness
#           tests/wayland/nested/harness.sh, Laufzeitordner unter $DICTATE_TEST_ROOT/wl-run)
#   gui     Einstellungsfenster im verschachtelten niri gegen den Mock, jede Seite schwebend in voller Größe
#           (tests/gui/smoke.sh), dazu die Bedienung der Seite Diagnose über den Injektor und die Antworten des Mocks
#           (tests/gui/diagnose.sh) und das Thema aus der Noctalia-Palette, dunkel und hell (tests/gui/theme.sh;
#           gemeinsame Teile tests/gui/lib.sh, Harness wie bei wayland)
#   speech  Vorlese-Testsatz über eine virtuelle Pulse-Quelle mit echtem Modell (tests/speech_testset.rs,
#           Schwellen und Mehrheitsregel in tests/speech/thresholds.tsv); ohne Aufnahmen übersprungen (Aufnahmen unter
#           $DICTATE_TEST_ROOT/audio/testset, übersteuert von DICTATE_TESTSET_DIR).
#           DICTATE_SPEECH_RUNS=1 scripts/check.sh --only speech: ein Lauf je Satz statt Mehrheit aus drei (schneller)
#   gpu     übrige GPU-/Pulse-Tests (engine_real, stream_real ohne bench_*, daemon_pulse_real, daemon_audio_real,
#           audio::tests::list_sources_real); Testaudio unter $DICTATE_TEST_ROOT/audio (nicht im Repo, fehlt es,
#           scheitert der Schritt)
#
# Testordner DICTATE_TEST_ROOT (scripts/test-root.sh, Vorgabe ${XDG_CACHE_HOME:-$HOME/.cache}/dictate-test, auf
# blub /home/data/stt-test): Logs, Sperre, Laufzeitordner des verschachtelten niri, Testaudio; das Skript legt ihn an
# und nennt ihn zu Beginn. Die Sperre liegt im Ordner: zwei Läufe mit verschiedenen Ordnern sperren sich nicht.
# Logs je Lauf unter $DICTATE_TEST_ROOT/logs/check/<zeit>/<schritt>.log (latest zeigt auf den letzten Lauf); im
# Terminal stehen nur Schritt, Ergebnis und Zeit, bei Fehlern das Ende des Logs. Builds mit CARGO_BUILD_JOBS=4 (Hitze),
# nie zwei Läufe gleichzeitig (Sperrdatei). Echte Sitzung (niri), dictate.service und die Standard-Quelle/-Senke
# bleiben unberührt; verschachteltes niri, Mocks, Test-Daemons und eigene Pulse-Module werden auch bei Abbruch beendet.
set -euo pipefail

cd "$(dirname "$0")/.."
ROOT=$PWD
export CARGO_BUILD_JOBS=4 CARGO_TERM_COLOR=never
# GUI-Programme nie auf dem echten Desktop öffnen: die Schritte wayland und gui starten sie nur im verschachtelten
# niri (tests/wayland/nested/harness.sh). Die echte Sitzung darum hier schon entfernen; ein Programm ohne eigenes
# Display findet so keins statt das echte (am 20.11. öffnete der damalige Schritt xephyr dictate-settings auf dem
# echten Desktop)
unset DISPLAY WAYLAND_DISPLAY WAYLAND_SOCKET NIRI_SOCKET SWAYSOCK

# shellcheck source=test-root.sh
source "$ROOT/scripts/test-root.sh"
SKIP=77

need() { command -v "$1" >/dev/null || { echo "FAILED: $1 is not installed (needed by this step)." >&2; return 1; }; }
# Gemeinsam für wayland und gui: Programme des Harness und der Injektor (eigener Workspace)
need_nested() {
  need sway && need niri && need grim && need xkbcli && need python3 || return 1
  python3 -c 'import PIL' || { echo "FAILED: python-pillow is needed (screenshots)"; return 1; }
  cargo build --locked --manifest-path tests/wayland/nested/Cargo.toml &&
    cargo test --locked --manifest-path tests/wayland/nested/Cargo.toml # Wächter (src/guard.rs)
}

step_fmt() { cargo fmt --check -- --color never; }
step_licenses() {
  need cargo-deny || return 1
  cargo deny --offline --color never check licenses && python3 scripts/third-party-licenses.py --check
}
step_clippy() { cargo clippy --release --all-targets -- -D warnings; }
step_unit() { cargo test --release; }
step_wayland() {
  need_nested || return 1
  python3 -c 'import gi; gi.require_version("Gtk", "4.0"); from gi.repository import Gtk' ||
    { echo "FAILED: python-gobject with GTK 4 is needed (test window)"; return 1; }
  cargo build --release --bin dictate-ui &&
    DICTATE_TEST_LOGS="$RUN/wayland" tests/wayland/run.sh
}
step_gui() {
  need_nested && cargo build --release --bin dictate-settings &&
    LOGS="$RUN/gui" tests/gui/smoke.sh "$ROOT/target/release/dictate-settings" &&
    LOGS="$RUN/gui" tests/gui/diagnose.sh "$ROOT/target/release/dictate-settings" &&
    LOGS="$RUN/gui" tests/gui/theme.sh "$ROOT/target/release/dictate-settings"
}
step_speech() {
  if ! compgen -G "$TESTSET_AUDIO/t*.wav" >/dev/null; then
    echo "SKIPPED: no recordings in $TESTSET_AUDIO (test folder DICTATE_TEST_ROOT=$DICTATE_TEST_ROOT; record them with scripts/record-testset.sh)"
    return $SKIP
  fi
  need pactl && need paplay &&
    DICTATE_SPEECH_OUT="$RUN/speech.tsv" cargo test --release --test speech_testset testset_over_pulse -- --ignored --nocapture --test-threads=1
}
step_gpu() {
  need pactl && need paplay && need sox || return 1
  # Eigene Nullsenke als Quelle für daemon_audio_real (wird nach dem Schritt sicher wieder entladen)
  local sink="dictate_check_$$"
  pactl load-module module-null-sink "sink_name=$sink" sink_properties=device.description=dictate-check >/dev/null
  DICTATE_TEST_SOURCE="$sink.monitor" cargo test --release --test engine_real --test stream_real --test daemon_pulse_real --test daemon_audio_real \
    -- --ignored --nocapture --test-threads=1 --skip bench_ &&
    cargo test --release --lib audio::tests::list_sources_real -- --ignored --exact
}

# name, in der Reihenfolge, in der sie laufen (billig zuerst)
steps=(fmt licenses clippy unit wayland gui speech gpu)

only=""
case "${1:-}" in
  "")
    [[ $# -eq 0 ]] || { echo "Empty argument (see the header of this script)" >&2; exit 2; }
    ;;
  --list)
    [[ $# -eq 1 ]] || { echo "--list takes no argument" >&2; exit 2; }
    printf '%s\n' "${steps[@]}"
    exit 0
    ;;
  --only)
    [[ $# -eq 2 && -n "${2//[, ]/}" ]] || { echo "--only needs one comma separated list of steps (see --list)" >&2; exit 2; }
    only="$2"
    IFS=, read -ra wanted <<<"$only"
    for w in "${wanted[@]}"; do
      [[ " ${steps[*]} " == *" $w "* ]] || { echo "Unknown step: $w (see --list)" >&2; exit 2; }
    done
    ;;
  *)
    echo "Unknown option: $1 (see the header of this script)" >&2
    exit 2
    ;;
esac

# Testordner erst nach den Argumenten auflösen (--list braucht ihn nicht); exportiert für Testskripte und cargo test
dictate_test_root || exit 2
LOGROOT=$DICTATE_TEST_ROOT/logs/check
TESTSET_AUDIO=${DICTATE_TESTSET_DIR:-$DICTATE_TEST_ROOT/audio/testset}
mkdir -p "$LOGROOT"
exec 9>"$LOGROOT/.lock"
flock -n 9 || { echo "FAILED: another scripts/check.sh is running (lock $LOGROOT/.lock)" >&2; exit 1; }
RUN="$LOGROOT/$(date +%Y%m%d-%H%M%S)"
mkdir -p "$RUN"
ln -sfn "$RUN" "$LOGROOT/latest"
echo "test folder: $DICTATE_TEST_ROOT (DICTATE_TEST_ROOT), logs: $RUN"
# Nur die letzten 20 Läufe behalten
ls -1dt "$LOGROOT"/2*/ 2>/dev/null | tail -n +21 | xargs -r rm -rf --

# Eigene Pulse-Module (Nullsenken der Tests und dieses Skripts), die vor dem Lauf schon da waren, gehören jemand
# anderem und bleiben.
# Ohne Pulse-Server (oder ohne pactl, etwa in der CI) gibt es keine eigenen Module; das darf unter pipefail nichts abbrechen
our_modules() { { pactl list short modules 2>/dev/null || true; } | awk '/sink_name=dictate_(e2e|check)_/ {print $1}'; }
modules_before=" $(our_modules | tr '\n' ' ') "

step_pid=""
cleanup() {
  local status=$?
  trap - EXIT INT TERM
  if [[ -n "$step_pid" ]] && kill -0 "$step_pid" 2>/dev/null; then
    # Der Schritt läuft in einer eigenen Prozessgruppe (set -m): alles darin beenden
    kill -TERM -- "-$step_pid" 2>/dev/null || true
    # Zeit zum Aufräumen geben (die Schritte wayland und gui bauen ihr verschachteltes niri selbst ab, dessen
    # Clients in eigenen Prozessgruppen laufen), höchstens 10 s
    local i
    for i in $(seq 100); do
      # die ganze Gruppe, nicht nur die Subshell: die stirbt sofort, ihr Testskript räumt dann noch auf
      kill -0 -- "-$step_pid" 2>/dev/null || break
      sleep 0.1
    done
    kill -KILL -- "-$step_pid" 2>/dev/null || true
  fi
  local m
  for m in $(our_modules); do
    [[ "$modules_before" == *" $m "* ]] || { pactl unload-module "$m" && echo "cleanup: unloaded pulse module $m"; }
  done
  exit "$status"
}
trap cleanup EXIT
trap 'echo; echo "interrupted"; exit 130' INT TERM

set -m
started=$SECONDS
ran=0
summary=()
for name in "${steps[@]}"; do
  if [[ -n "$only" && ",$only," != *",$name,"* ]]; then
    continue
  fi
  log="$RUN/$name.log"
  printf '==> %-8s ' "$name"
  t0=$SECONDS
  # Eigene Prozessgruppe und Subshell mit eigenem `set -e`: ein Schritt, der halb scheitert, hält dort an
  ( set -e; "step_$name" ) >"$log" 2>&1 </dev/null &
  step_pid=$!
  set +e
  wait "$step_pid"
  status=$?
  set -e
  step_pid=""
  # Nach jedem Schritt: übrig gebliebene eigene Pulse-Module entladen (Tests, die abstürzen, kommen nicht mehr dazu)
  for m in $(our_modules); do
    [[ "$modules_before" == *" $m "* ]] || { pactl unload-module "$m"; echo "cleanup: unloaded pulse module $m" >>"$log"; }
  done
  secs=$((SECONDS - t0))
  if [[ "$status" -eq $SKIP ]]; then
    echo "skipped ($secs s): $(tail -1 "$log")"
    summary+=("$(printf '%-8s skipped %5d s' "$name" "$secs")")
    continue
  fi
  if [[ "$status" -ne 0 ]]; then
    echo "FAILED ($secs s), log: $log"
    tail -n 30 "$log" | sed 's/^/    /'
    summary+=("$(printf '%-8s FAILED  %5d s' "$name" "$secs")")
    printf '%s\n' "${summary[@]}"
    exit 1
  fi
  extra=""
  [[ "$name" == speech ]] && extra=" — $(grep -m1 '^total CER' "$log" || true)"
  echo "ok ($secs s)$extra"
  summary+=("$(printf '%-8s ok      %5d s' "$name" "$secs")")
  ran=$((ran + 1))
done
if [[ "$ran" -eq 0 && ${#summary[@]} -eq 0 ]]; then
  echo "FAILED: no step was run." >&2
  exit 1
fi
echo "---"
printf '%s\n' "${summary[@]}"
echo "All checks passed ($ran ok, $((${#summary[@]} - ran)) skipped, $((SECONDS - started)) s). Logs: $RUN"
