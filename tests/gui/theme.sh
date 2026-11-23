#!/usr/bin/env bash
# Copyright (C) 2026 Daniel Kurdoghlian
# Pushing Pixels, Vienna <https://pushingpixels.at>
# SPDX-License-Identifier: GPL-3.0-or-later
# Thema des Einstellungsfensters aus der Noctalia-Palette (W6 Block C, PLAN-wayland.md 6h) im verschachtelten niri
# gegen den Mock (tests/gui/lib.sh): das Fenster liest die Palette aus DICTATE_PALETTE (Kopie der Fixture im
# Laufzeitordner, nie die Fixture im Repo, nie Daniels Datei), ein Pixel der Seitenleiste hat die Rolle
# `surface_container_low`, eines im Inhalt `surface`; erwartet wird die Farbe aus dem JSON, nicht ein fester Wert.
# Erst dunkel beim Start; dann im laufenden Fenster (Lesart L43): eine andere Datei im selben Ordner (frecency.json)
# färbt nicht um, die Kopie per rename durch die helle ersetzt färbt um (Messpunkt M2: Zeile „theme: palette applied
# after N ms“, Ziel unter 200 ms, Zahl nach $LOGS/theme-m2.txt), Löschen fällt auf Tokyo Night dunkel zurück; zuletzt
# Neustart mit der hellen Kopie (Hell/Dunkel nur aus `mode`, Vorlage V2). Baut niri, Mock und Fenster auch bei Abbruch
# ab.
# Aufruf: theme.sh BINARY        Umgebung: LOGS (Screenshots und Logs)
set -u
BIN=$1
# shellcheck source=lib.sh
source "$(dirname "$0")/lib.sh"
GUI_PALETTE=dark
gui_up || exit 1

# Farbe der Rolle ROLLE aus der Palette des Laufs (bzw. aus DATEI)
role() { python3 -c 'import json, sys; print(json.load(open(sys.argv[1]))[sys.argv[2]])' "${2:-$DICTATE_PALETTE}" "$1"; }

# Pixel an logischen Fensterkoordinaten X Y im Bild DATEI als #rrggbb
pixel_of() { # DATEI X Y
    python3 - "$1" "$2" "$3" "$NESTED_SCALE" <<'PY'
import sys
from PIL import Image
img, x, y, s = sys.argv[1], *map(float, sys.argv[2:])
r, g, b = Image.open(img).convert("RGB").getpixel((round(x * s), round(y * s)))
print(f"#{r:02x}{g:02x}{b:02x}")
PY
}

# Pixel gegen #rrggbb (Abweichung je Kanal höchstens 3)
pixel_is() { # DATEI X Y HEX BESCHREIBUNG
    local got
    got=$(pixel_of "$1" "$2" "$3")
    if python3 -c 'import sys
a, b = sys.argv[1].lstrip("#"), sys.argv[2].lstrip("#")
sys.exit(0 if all(abs(int(a[i:i + 2], 16) - int(b[i:i + 2], 16)) <= 3 for i in (0, 2, 4)) else 1)' "$got" "$4"; then
        echo "ok    $5: $got"
    else
        echo "FAIL  $5: $got, expected $4"
        fail=$((fail + 1))
    fi
}

check_colors() { # NAME [PALETTE]: Fenster läuft, Farben gegen die Palette des Laufs (bzw. PALETTE)
    local shot=$LOGS/theme-$1.png pal=${2:-$DICTATE_PALETTE} want i
    want=$(role surface_container_low "$pal")
    # Die Logzeile kommt vor der Übergabe an den Compositor: bis zu 3 s auf den umgefärbten Frame warten
    for i in $(seq 1 15); do
        gshot "$shot" || { echo "FAIL  $1: no screenshot"; fail=$((fail + 1)); return; }
        [[ $(pixel_of "$shot" 6 400) == "$want" ]] && break
        sleep 0.2
    done
    # x 6 liegt links der eingerückten Zeilen (12 px), y 400 mitten in der Liste: nur die Fläche der Seitenleiste
    pixel_is "$shot" 6 400 "$want" "$1: sidebar is surface_container_low"
    # rechts neben dem Inhalt (höchstens 720 breit ab 288): nur die Fläche des Inhalts
    pixel_is "$shot" 1160 400 "$(role surface "$pal")" "$1: content is surface"
}

log_has() { # TEXT BESCHREIBUNG [SEKUNDEN]: feste Zeichenkette im Log des Fensters
    if nested_wait "${3:-3}" grep -qF -- "$1" "$GUI_LOG"; then
        echo "ok    $2"
    else
        echo "FAIL  $2: no '$1' in the log"
        fail=$((fail + 1))
    fi
}

applied() { grep -c 'dictate-settings: theme: palette applied after' "$GUI_LOG"; }
applied_twice() { (($(applied) >= 2)); }

m2() { # BESCHREIBUNG: Zahl aus der letzten M2-Zeile
    local n
    n=$(grep -o 'theme: palette applied after [0-9]* ms' "$GUI_LOG" | tail -n1 | grep -o '[0-9][0-9]*')
    echo "M2 $1: ${n:-?} ms (goal under 200)" | tee -a "$LOGS/theme-m2.txt"
}

DARK_FX=$GUI_HERE/fixtures/palette-dark.json
: >"$LOGS/theme-m2.txt"
gui_start -- "$BIN" --page general || exit 1
check_colors dark
log_has "dictate-settings: palette from $DICTATE_PALETTE (dark)" "dark: palette read from the file"
# Schrift aus der Fixture settings.ini (Lesart L41): Familie ohne Größe an fc-match, 12 pt = 16 px, Titel 600
log_has "dictate-settings: font Roboto 16.00 px from $XDG_CONFIG_HOME/gtk-4.0/settings.ini (base Roboto-Regular.ttf, title Roboto-Bold.ttf)" "font from settings.ini"

# Eine andere Datei im selben Ordner (wie frecency.json des Daemons): kein Umfärben, kein neues Lesen
before=$(applied)
printf '{"version": 1, "entries": {}}\n' >"$(dirname "$DICTATE_PALETTE")/frecency.json"
sleep 0.5
if [[ $(applied) == "$before" && $(grep -c 'dictate-settings: palette' "$GUI_LOG") == 1 ]]; then
    echo "ok    another file in the folder changes nothing"
else
    echo "FAIL  another file in the folder recolored or reread the palette"
    fail=$((fail + 1))
fi

# Im laufenden Fenster durch die helle ersetzen (per rename wie Noctalia)
cp "$GUI_HERE/fixtures/palette-light.json" "$DICTATE_PALETTE.tmp" && mv "$DICTATE_PALETTE.tmp" "$DICTATE_PALETTE"
log_has "dictate-settings: theme: palette applied after" "live: recolored after the rename"
log_has "dictate-settings: palette from $DICTATE_PALETTE (light)" "live: light palette read from the file"
m2 "rename to light"
check_colors live-light

# Löschen: Rückfall auf die eingebaute Palette (Tokyo Night dunkel), Grund im Log
rm "$DICTATE_PALETTE"
log_has "dictate-settings: palette: built-in Tokyo Night" "deleted: falls back to the built-in palette"
if nested_wait 3 applied_twice; then
    echo "ok    deleted: recolored"
else
    echo "FAIL  deleted: not recolored"
    fail=$((fail + 1))
fi
m2 "delete (fallback)"
check_colors live-fallback "$DARK_FX"
gui_stop

# Neustart mit der hellen Kopie
cp "$GUI_HERE/fixtures/palette-light.json" "$DICTATE_PALETTE.tmp" && mv "$DICTATE_PALETTE.tmp" "$DICTATE_PALETTE"
gui_start -- "$BIN" --page general || exit 1
check_colors light
log_has "dictate-settings: palette from $DICTATE_PALETTE (light)" "light: palette read from the file"
gui_stop

if ((fail)); then
    echo "theme: $fail problem(s)"
    exit 1
fi
echo "theme: ok"
