#!/usr/bin/env bash
# Copyright (C) 2026 Daniel Kurdoghlian
# Pushing Pixels, Vienna <https://pushingpixels.at>
# SPDX-License-Identifier: GPL-3.0-or-later
# Bedienung von Hand nachgestellt (zum Ansehen, nichts geprüft außer Absturz und Abbau): Seitenwechsel, Pegel, Wert
# ändern, Apply (Fehler und Erfolg), Seite Model. Im verschachtelten niri gegen den Mock (tests/gui/lib.sh), Klickziele
# aus dem Lage-Log des Fensters (fehlt eines, zählt das als Problem). Nicht Teil von scripts/check.sh.
# Aufruf: interact.sh BINARY [PREFIX]     Screenshots nach PREFIX-i-*.png (Vorgabe $DICTATE_TEST_ROOT/logs/v2b)
set -u
BIN=$1
# shellcheck source=lib.sh
source "$(dirname "$0")/lib.sh"
PFX=${2:-${DICTATE_TEST_ROOT:-}/logs/v2b}
gui_up || exit 1
shot() { gshot "$PFX-$1.png"; }
gui_start -- "$BIN" --page general || exit 1
gclick_id nav:audio
sleep 1.5
shot i-audio-level
gclick_id nav:ai
gui_reveal ai.timeout_ms
gclick_id ai.timeout_ms
gkey ctrl+a
gtype 110000
gkey return
gclick_id apply # Fehler erwartet
sleep 1
shot i-ai-error
gclick_id ai.timeout_ms
gkey ctrl+a
gtype 6000
gkey return
gclick_id apply # ok
sleep 1
shot i-ai-applied
gclick_id nav:model
sleep 1
shot i-model
gui_stop
echo "== $fail problem(s), screenshots: $PFX-i-*.png =="
[ "$fail" -eq 0 ]
