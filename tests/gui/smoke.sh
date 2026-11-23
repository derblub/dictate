#!/usr/bin/env bash
# Copyright (C) 2026 Daniel Kurdoghlian
# Pushing Pixels, Vienna <https://pushingpixels.at>
# SPDX-License-Identifier: GPL-3.0-or-later
# Rauchtest des Einstellungsfensters im verschachtelten niri gegen den Mock (tests/gui/lib.sh): je Seite ein Start mit
# --page; das Fenster muss schwebend in voller Größe erscheinen (Fensterregel aus niri/dictate-binds.kdl), der Prozess
# darf nicht abstürzen (kein „panicked“), die Seite Audio muss beim Mock den Live-Pegel einschalten, und jede Seite
# öffnet ohne Änderung (Apply gesperrt, kein set_config; About hat kein Apply). Klickziele aus dem Lage-Log des
# Fensters (lib.sh). Je Seite ein Screenshot nach $LOGS/gui-<seite>.png (zum
# Ansehen, nicht geprüft). Baut niri, Mock und Fenster auch bei Abbruch ab.
# Aufruf: smoke.sh BINARY        Umgebung: LOGS (Screenshots und Logs)
set -u
BIN=$1
# shellcheck source=lib.sh
source "$(dirname "$0")/lib.sh"
gui_up || exit 1
for p in general audio model recognition overlay voice vocabulary profiles ai actions palette diagnose history about; do
  before=$fail
  if gui_start -- "$BIN" --page "$p"; then
    gshot "$LOGS/gui-$p.png"
    kill -0 "$GUI_PID" 2>/dev/null || { echo "FAIL  $p: GUI exited"; fail=$((fail + 1)); }
    if [ "$p" = audio ] && ! grep -q '"cmd": "meter", "on": true' "$MOCK_LOG" 2>/dev/null; then
      echo "FAIL  $p: meter not switched on at the mock"
      fail=$((fail + 1))
    fi
    if [ "$p" = about ]; then
      # About hat keine Aktionsleiste: dort darf es kein Apply geben
      if [ -n "$(gui_spot apply)" ]; then
        echo "FAIL  $p: has an Apply button"
        fail=$((fail + 1))
      fi
    else
      # Die Seite öffnet ohne Änderung: Apply ist gesperrt, ein Klick darauf schickt nichts (ein Formular, das beim
      # Zeichnen Vorgaben ergänzt, stand sonst gleich mit „Unsaved changes“ da; Mock wie ein älterer Daemon, siehe
      # dort). Ziel aus dem Lage-Log; fehlt es, zählt das als Fehler (sonst bestünde der Test ohne Klick).
      if gclick_id apply; then
        sleep 0.5
        if grep -q '"cmd": "set_config"' "$MOCK_LOG" 2>/dev/null; then
          echo "FAIL  $p: opened with unsaved changes (Apply sent set_config)"
          fail=$((fail + 1))
        fi
      fi
    fi
  fi
  gui_stop
  [ "$fail" -eq "$before" ] && echo "PASS  $p" || echo "FAIL  $p"
done
echo "== $fail problem(s), screenshots: $LOGS/gui-*.png =="
[ "$fail" -eq 0 ]
