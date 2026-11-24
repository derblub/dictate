#!/usr/bin/env bash
# Copyright (C) 2026 Daniel Kurdoghlian
# Pushing Pixels, Vienna <https://pushingpixels.at>
# SPDX-License-Identifier: GPL-3.0-or-later
# Nimmt den Vorlese-Testsatz (tests/speech/testset.tsv) Satz für Satz vom Mikrofon auf.
# Je Satz: Enter startet, Enter stoppt; danach Enter = behalten, r = nochmal, s = überspringen, q = beenden.
# Ausgabe: $OUT/<id>.wav (16 kHz mono s16) und $OUT/<id>.txt (Vorlesetext ohne Regieanweisungen).
# Aufruf: scripts/record-testset.sh [id …]     (ohne ids: alle; vorhandene Aufnahmen werden nur auf Nachfrage ersetzt)
# Umgebung: DICTATE_TEST_SOURCE (Pulse-Quelle, Standard: Default-Quelle), DICTATE_TESTSET_OUT (Zielordner, Standard
# $DICTATE_TEST_ROOT/audio/testset; Testordner aus scripts/test-root.sh, dort liest ihn auch scripts/check.sh).
# Braucht ein echtes Terminal: ohne läuft nichts (ein Aufruf ohne TTY legte früher leere Dateien an). Aufnahmen unter
# MIN_SECS werden nie behalten, sondern zur Wiederholung angeboten.
set -euo pipefail
if [ ! -t 0 ] || ! { exec 3</dev/tty; } 2>/dev/null; then
  echo "record-testset.sh: kein Terminal (stdin ist kein TTY oder /dev/tty nicht lesbar), breche ab." >&2
  echo "Bitte direkt in einem Terminal aufrufen, nicht aus einem Skript oder Agenten." >&2
  exit 1
fi
exec 3<&-
MIN_SECS=0.5
HERE="$(cd "$(dirname "$0")/.." && pwd)"
SET="$HERE/tests/speech/testset.tsv"
# shellcheck source=test-root.sh
source "$HERE/scripts/test-root.sh"
if [[ -z ${DICTATE_TESTSET_OUT:-} ]]; then
  dictate_test_root || exit 1
fi
OUT="${DICTATE_TESTSET_OUT:-$DICTATE_TEST_ROOT/audio/testset}"
SOURCE="${DICTATE_TEST_SOURCE:-$(pactl get-default-source)}"
mkdir -p "$OUT"
want=" $* "

echo "Quelle: $SOURCE"
echo "Ziel:   $OUT"
echo "Sprich normal laut wie beim Diktieren (ausser bei Regieanweisungen in [eckigen Klammern])."
echo "Nach Enter etwa eine halbe Sekunde warten, bevor du sprichst, und nach dem letzten Wort kurz warten."
echo

rec_pid=""
cleanup() { [ -n "$rec_pid" ] && kill "$rec_pid" 2>/dev/null; wait 2>/dev/null || true; }
trap cleanup EXIT INT TERM

while IFS=$'\t' read -r id what text expected; do
  case "$id" in ''|'#'*|id) continue ;; esac
  [ $# -gt 0 ] && [[ "$want" != *" $id "* ]] && continue
  if [ -f "$OUT/$id.wav" ] && [ $# -eq 0 ]; then
    read -r -p "$id existiert schon. Neu aufnehmen? [j/N] " a </dev/tty
    [ "$a" = j ] || continue
  fi
  while true; do
    printf '\n\033[1m%s\033[0m  (%s)\n\n    %s\n\n' "$id" "$what" "$text"
    read -r -p "Enter = Aufnahme starten " _ </dev/tty
    tmp="$OUT/.$id.wav"
    parecord --device="$SOURCE" --latency-msec=50 --rate=16000 --channels=1 --format=s16le --file-format=wav "$tmp" </dev/null &
    rec_pid=$!
    read -r -p "● Aufnahme läuft … Enter = stoppen " _ </dev/tty
    kill -INT "$rec_pid" 2>/dev/null; wait "$rec_pid" 2>/dev/null || true; rec_pid=""
    # Länge in s (0, wenn parecord nichts Lesbares geschrieben hat); kurz = unter MIN_SECS
    read -r secs short < <(python3 -I -c '
import sys, wave
try:
    w = wave.open(sys.argv[1]); s = w.getnframes() / w.getframerate()
except Exception:
    s = 0.0
print("%.1f %d" % (s, s < float(sys.argv[2])))' "$tmp" "$MIN_SECS")
    if [ "$short" = 1 ]; then
      rm -f "$tmp"
      read -r -p "Nur $secs s aufgenommen (unter $MIN_SECS s), nicht behalten. Enter/r = nochmal, s = überspringen, q = beenden: " a </dev/tty
      case "$a" in
        s) break ;;
        q) exit 0 ;;
        *) continue ;;
      esac
    fi
    read -r -p "$secs s aufgenommen. Enter = behalten, r = nochmal, s = überspringen, q = beenden: " a </dev/tty
    case "$a" in
      r) rm -f "$tmp"; continue ;;
      s) rm -f "$tmp"; break ;;
      q) rm -f "$tmp"; exit 0 ;;
      *) mv -f "$tmp" "$OUT/$id.wav"
         printf '%s\n' "$text" | sed -E 's/\[[^]]*\] ?//g' > "$OUT/$id.txt"
         echo "gespeichert: $OUT/$id.wav"; break ;;
    esac
  done
done < "$SET"
echo
echo "Fertig. Aufnahmen: $(ls "$OUT"/*.wav 2>/dev/null | wc -l) in $OUT"
