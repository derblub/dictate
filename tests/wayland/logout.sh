# Copyright (C) 2026 Daniel Kurdoghlian
# Pushing Pixels, Vienna <https://pushingpixels.at>
# SPDX-License-Identifier: GPL-3.0-or-later
# Abmelden aus der Befehlspalette (Block W5, PLAN-wayland.md 6g, Teil D2), eingebunden von tests/wayland/run.sh als letzter
# Test nach lock.sh: `logout` schickt niri `Quit` und beendet damit das verschachtelte niri (nie das echte: dictate-ui spricht
# nur mit `NIRI_SOCKET` im eigenen Laufzeitordner). Danach spricht kein Helfer mehr mit niri (`nmsg`), nur noch die
# Log-Prüfung liest Dateien. Prüft: das erste Return zeigt nur die Bestätigung (niri läuft, keine Wahl), das zweite beendet
# niri, `pal_chosen logout` kommt vorher an (L14: die Wahl geht vor `Quit`, danach wäre die Sitzung fort). Kinder von niri
# (xwayland-satellite) sammelt der Test selbst ein: nach dem Ende von niri fände nested_down sie nicht mehr.

LOGOUT_KIDS=$(pgrep -P "$NESTED_NIRI_PID" | tr '\n' ' ')
focus dictate-test-target || bad "logout: focus the target"
c0=$(pal_chosen_n logout)
logout_not_yet() {
    check "logout: the first Return only asks (niri runs, no choice yet)" bash -c "kill -0 $NESTED_NIRI_PID 2>/dev/null && (( \$(grep -c ' pal_chosen logout\$' '$MOCK_LOG') == $c0 ))"
}
PAL_BEFORE_CONFIRM=logout_not_yet pal_run qqlogout confirm || bad "logout: run"
niri_gone() { ! kill -0 "$NESTED_NIRI_PID" 2>/dev/null; }
check "logout: the second Return ends the nested niri" nested_wait 10 niri_gone
t_end=$(now_ns)
check "logout: pal_chosen logout arrived" more 'pal_chosen logout' "$c0"
check "logout: pal_chosen came before niri ended (L14)" test "$(last_ns 'pal_chosen logout')" -lt "$t_end"
if nested_wait 10 bash -c "! kill -0 $UI_PID 2>/dev/null"; then
    echo "info logout: dictate-ui ended with the session"
    UI_PID=""
else
    echo "info logout: dictate-ui still runs after the session ended (cleanup ends it)"
fi
for k in $LOGOUT_KIDS; do kill -0 "$k" 2>/dev/null && kill -TERM "$k"; done
