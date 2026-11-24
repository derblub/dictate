#!/usr/bin/env bash
# Copyright (C) 2026 Daniel Kurdoghlian
# Pushing Pixels, Vienna <https://pushingpixels.at>
# SPDX-License-Identifier: GPL-3.0-or-later
# Seite Diagnose des Einstellungsfensters im verschachtelten niri gegen den Mock (tests/gui/lib.sh): Bedienung über den
# Injektor und Prüfung dessen, was der Mock als set_config bekommt und beantwortet (mock.log).
#  1. Ausgeschaltet (Vorgaben): einschalten, Apply -> diagnose.enabled ist true, alle anderen Felder unverändert.
#  2. Ping-Ziele: Ziel hinzufügen und Apply (drei Ziele); einen ungültigen Namen eintragen -> der Mock lehnt das Feld
#     `diagnose.ping_targets[2].name` ab; Ziel entfernen und Apply -> wieder zwei.
#  3. Datei mit unzulässigen Werten (MOCK_DIAGNOSE_BAD): Ordner, Ziele rot schon beim Anzeigen (Screenshot).
#  4. Cloud-Server (MOCK_CLOUD): Hinweis auf den Anbieter und das Vorgabemodell (Screenshot).
# Klickziele aus dem Lage-Log des Fensters (lib.sh, src/gui/layout_log.rs); feste Rasten nur für die Bildausschnitte.
# Screenshots nach $LOGS/diagnose-*.png (zum Ansehen, nicht geprüft). Baut niri, Mock und Fenster auch bei Abbruch ab.
# Aufruf: diagnose.sh BINARY        Umgebung: LOGS (Screenshots und Logs)
set -u
BIN=$1
# shellcheck source=lib.sh
source "$(dirname "$0")/lib.sh"
gui_up || exit 1
waitfor() { gui_waitfor "$@"; }
check() { # name, ist, soll
  if [ "$2" = "$3" ]; then echo "PASS  $1"; else echo "FAIL  $1 (got: $2, expected: $3)"; fail=$((fail + 1)); fi
}

# Startet die Seite gegen den Mock mit den Argumenten als Umgebung (NAME=WERT) und wartet auf das Fenster.
start() { gui_start "$@" -- "$BIN" --page diagnose; }
stop() { gui_stop; }
scroll() { gscroll "$1"; } # Anzahl Rasten nach unten (negativ: nach oben)
shot() { gshot "$LOGS/diagnose-$1.png"; }
# Zeile des Mock-Logs: die letzte Anfrage set_config als JSON des Abschnitts [diagnose], die Antworten als „ok“/Feld
last_diagnose() { python3 -c '
import json, sys
reqs = [json.loads(l) for l in open(sys.argv[1]) if l.strip()]
sets = [r for r in reqs if r["cmd"] == "set_config"]
print(json.dumps(sets[-1]["config"]["diagnose"], sort_keys=True) if sets else "none")' "$MOCK_LOG" 2>/dev/null; }
results() { python3 -c '
import json, sys
reqs = [json.loads(l) for l in open(sys.argv[1]) if l.strip()]
print(" ".join(("ok" if r["ok"] else r["field"]) for r in reqs if r["cmd"] == "set_config_result"))' "$MOCK_LOG" 2>/dev/null; }
n_results() { results | wc -w; }
apply() { # klickt Apply und wartet auf die Antwort des Mocks
  local before
  before=$(n_results)
  gclick_id apply || return 1
  sleep 0.2
  waitfor "[ \$(n_results) -gt $before ]" 5
}
click_id() { gclick_id "$1" && sleep 0.2; }
T3=diagnose.ping_targets[2]
# Ausdruck über den Abschnitt `d` (Python) als JSON
dj() { python3 -c 'import json, sys; d = json.loads(sys.argv[1]); print(json.dumps(eval(sys.argv[2]), sort_keys=True))' "$1" "$2"; }

# 1. Ausgeschaltet, Vorgaben
if start MOCK_DIAGNOSE_OFF=1; then
  shot off-top
  click_id feature_switch # Chip Disabled -> Enabled
  apply
  d=$(last_diagnose)
  check "off: Apply is accepted" "$(results)" ok
  check "off: the chip switches it on" "$(dj "$d" 'd["enabled"]')" true
  check "off: nothing else changed" "$(dj "$d" '[d["rounds"], d["report_kib"], d["timeout_ms"], d["pseudonymize"], d["boot_hint"], d["ping_targets"], d["model"]]')" '[3, 48, 90000, true, true, [], ""]'
  shot on-top
fi
stop

# 2. Ping-Ziele: zwei Ziele aus dem Mock; Ansicht „Ping targets“ (Ende der Seite, 16 Rasten zurück, nur für das Bild;
#    die Klickziele kommen aus dem Lage-Log, gui_reveal scrollt nach, falls „Add target“ nicht ganz sichtbar ist)
if start; then
  scroll 80
  scroll -16
  shot targets
  gui_reveal diagnose.add_target
  click_id diagnose.add_target # die Zeile entsteht an dieser Stelle, der Knopf rückt darunter
  click_id "$T3.name"          # Name der neuen Zeile
  gtype router
  gkey tab
  gtype 192.0.2.254
  sleep 0.4
  shot targets-added
  apply
  d=$(last_diagnose)
  check "targets: Apply is accepted" "$(results)" ok
  check "targets: Add target, name and address reach the mock" "$(dj "$d" '[[t["name"], t["address"]] for t in d["ping_targets"]]')" \
    '[["gateway", "192.0.2.1"], ["nas", "nas.example.org"], ["router", "192.0.2.254"]]'
  # ungültiger Name: das Fenster zeigt es schon beim Tippen rot, der Mock lehnt das Feld ab
  click_id "$T3.name"
  gkey ctrl+a
  gtype -bad
  sleep 0.4
  shot targets-bad
  apply
  check "targets: the invalid name is rejected at its field" "$(results)" "ok diagnose.ping_targets[2].name"
  sleep 0.4
  shot targets-rejected
  click_id "$T3.remove" # Remove der dritten Zeile
  apply
  d=$(last_diagnose)
  check "targets: Remove and Apply are accepted" "$(results)" "ok diagnose.ping_targets[2].name ok"
  check "targets: the two targets are left" "$(dj "$d" '[t["name"] for t in d["ping_targets"]]')" '["gateway", "nas"]'
  check "targets: the third row is gone from the window" "$(gui_spot "$T3.name")" gone
  shot targets-removed
fi
stop

# 3. Datei mit unzulässigen Werten
if start MOCK_DIAGNOSE_BAD=1; then
  scroll 80
  scroll -16
  shot bad-targets
  scroll -6
  shot bad-folder
fi
stop

# 4. Cloud-Server: Hinweis auf den Anbieter und das Vorgabemodell
if start MOCK_CLOUD=1; then
  scroll 12
  shot cloud-ai
fi
stop

echo "== $fail check(s) failed, screenshots: $LOGS/diagnose-*.png =="
[ "$fail" -eq 0 ]
