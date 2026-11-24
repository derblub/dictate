#!/usr/bin/env bash
# Copyright (C) 2026 Daniel Kurdoghlian
# Pushing Pixels, Vienna <https://pushingpixels.at>
# SPDX-License-Identifier: GPL-3.0-or-later
# Screenshots aller Seiten (oben und 15 Rasten nach unten gescrollt): je Seite ein Start mit --page, im verschachtelten
# niri gegen den Mock (tests/gui/lib.sh). Exportierte MOCK_CLOUD=1 usw. erben Mock
# und Fenster.
# Aufruf: shots.sh BINARY [PREFIX]     Bilder nach PREFIX-<seite>.png und PREFIX-<seite>-2.png
#         (Vorgabe $DICTATE_TEST_ROOT/logs/v2b-pp)
set -u
BIN=$1
# shellcheck source=lib.sh
source "$(dirname "$0")/lib.sh"
PFX=${2:-${DICTATE_TEST_ROOT:-}/logs/v2b-pp}
gui_up || exit 1
for p in general audio model recognition overlay voice vocabulary profiles ai actions palette diagnose history about; do
  if gui_start -- "$BIN" --page "$p"; then
    gshot "$PFX-$p.png"
    gscroll 15
    gshot "$PFX-$p-2.png"
  fi
  gui_stop
done
echo "== $fail problem(s), screenshots: $PFX-*.png =="
[ "$fail" -eq 0 ]
