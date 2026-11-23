# Copyright (C) 2026 Daniel Kurdoghlian
# Pushing Pixels, Vienna <https://pushingpixels.at>
# SPDX-License-Identifier: GPL-3.0-or-later
# Prüfungen der Sperr-Erkennung von dictate-ui (Block W3c, PLAN-wayland.md 3.6, Regel 7, Befund 11), eingebunden von
# tests/wayland/run.sh nach actions.sh (nutzt dessen Helfer). Gegen das falsche logind auf dem privaten System-Bus
# (tests/wayland/fake-logind; eigene Sitzung _32 = Nummer 2 bzw. "auto", fremde _33): LockedHint, Signal Lock und
# PrepareForSleep(true) der eigenen Sitzung schließen ein offenes Panel und löschen act_* und diag_* im Daemon, auch mitten in
# der Arbeit (act_cancel) und bei geschlossenem Panel; fremde Sitzung, Entsperren und PrepareForSleep(false) tun nichts.
# Danach Neustart mit unbekannter XDG_SESSION_ID (Rückfall auf "auto"), mit veralteter Sitzung, mit schon gesetztem
# LockedHint und ohne System-Bus (Hinweis im Panel). Diagnose (W4, nach diagnose.sh, nutzt dessen Helfer): die Sperre
# schließt das Diagnose-Panel und löscht, mitten im Sammeln aufgelegt; Mod+Shift+I bei Sperre verweigert mit Hinweis;
# ohne Erkennung der Sperre der Hinweis auch im Diagnose-Panel. Befehlspalette (W5, nach palette.sh, nutzt dessen
# Helfer): die Sperre schließt sie, legt eine laufende KI-Anfrage auf und bricht eine Aufnahme der Palette ab (danach
# gehört das Diktat wieder dem Fenster); Mod+Alt+Space bei Sperre verweigert mit Hinweis; ohne Erkennung der Hinweis
# auch in der Palette.

logind_test() { # METHODE ARG...: Steuerung des falschen logind (nur der private Bus)
    [[ $DBUS_SYSTEM_BUS_ADDRESS == "unix:path=$NESTED_RT/"* ]] || return 70
    gdbus call --address "$DBUS_SYSTEM_BUS_ADDRESS" -d org.freedesktop.login1 -o /org/dictate/test -m "org.dictate.Test.$1" "${@:2}" >/dev/null
}
# Signal von einem anderen Client am Bus (nicht vom Namen org.freedesktop.login1): darf nie zählen
foreign_emit() { # PFAD SIGNAL [ARG…]
    [[ $DBUS_SYSTEM_BUS_ADDRESS == "unix:path=$NESTED_RT/"* ]] || return 70
    gdbus emit --address "$DBUS_SYSTEM_BUS_ADDRESS" --object-path "$1" --signal "$2" "${@:3}" >/dev/null
}
locks() { grep -c 'session locked' "$UI_LOG"; }
more_locks() { (($(locks) > $1)); }
# Eine Sperre: Panel zu (falls offen), act_clear und diag_clear je einmal mehr
cleared_after() { # ACT_CLEAR DIAG_CLEAR
    nested_wait 5 more_acts act_clear "$1" && nested_wait 5 more_acts diag_clear "$2"
}

# --- 27. Erkennung beim Start: eigene Sitzung über XDG_SESSION_ID ---------------------------------------------------
# eigenes Fenster mit Markierung (PRIMARY gehört dem zuletzt markierenden Fenster; actions.sh hat es weitergegeben)
nested_spawn lk python3 "$HERE/target.py" text dictate-test-lock "$SEL"
nested_wait 10 has_win dictate-test-lock || bad "lock: selection window"
nested_wait 5 grep -q '^selected' "$NESTED_DIR/lk.log"
check "lock: GetSession with XDG_SESSION_ID first" test "$(head -n 1 "$LOGIND_CALLS")" = 2
check "lock: watching the own session" grep -q 'lock: watching logind session /org/freedesktop/login1/session/_32 (from "2")' "$UI_LOG"
check "lock: no warning at start" bash -c "! grep -q 'lock: .*\(failed\|cannot\|no system bus\|not found\)' '$UI_LOG'"

# --- 28. Die drei Auslöser bei offenem Panel ---------------------------------------------------------------------
for trig in hint lock sleep; do
    open_panel dictate-test-lock || bad "lock ($trig): panel open"
    c0=$(acts act_clear) d0=$(acts diag_clear) l0=$(locks)
    case $trig in
    hint) logind_test SetLockedHint _32 true ;;
    lock) logind_test Lock _32 ;;
    sleep) logind_test Sleep true ;;
    esac
    check "lock ($trig): detected" nested_wait 5 more_locks "$l0"
    check "lock ($trig): panel closes" nested_wait 5 panel_down
    check "lock ($trig): act_clear and diag_clear" cleared_after "$c0" "$d0"
    case $trig in
    hint) logind_test SetLockedHint _32 false ;;
    sleep) logind_test Sleep false ;;
    esac
done
# --- 28b. Diagnose offen: die Sperre schließt sie und löscht; mitten im Sammeln wird aufgelegt ----------------------------
focus dictate-test-lock
diag_open || bad "lock (diagnose): panel open"
c0=$(acts act_clear) d0=$(acts diag_clear) l0=$(locks)
logind_test Lock _32
check "lock (diagnose): detected" nested_wait 5 more_locks "$l0"
check "lock (diagnose): panel closes" nested_wait 5 panel_down
check "lock (diagnose): act_clear and diag_clear" cleared_after "$c0" "$d0"
touch "$D/slow_diag"
focus dictate-test-lock
diag_open || bad "lock (diagnose, collecting): panel open"
diag_items
h0=$(acts "diag_collect hung up") c0=$(acts act_clear) d0=$(acts diag_clear) p0=$(renders Progress)
key return
nested_wait 5 more_renders Progress "$p0"
sleep 0.5
logind_test Lock _32
check "lock (diagnose, collecting): panel closes" nested_wait 5 panel_down
check "lock (diagnose, collecting): the running collection hung up" nested_wait 5 more_acts "diag_collect hung up" "$h0"
check "lock (diagnose, collecting): act_clear and diag_clear" cleared_after "$c0" "$d0"
rm -f "$D/slow_diag"

# --- 28c. Palette offen (W5): die Sperre schließt sie, legt die KI auf und bricht eine Aufnahme der Palette ab ----------
rm -f "$D/late_final" "$D/pal_off" "$D/pal_slow" "$D/pal_slow_ask"
focus dictate-test-lock
pal_open || bad "lock (palette): open"
c0=$(acts act_clear) d0=$(acts diag_clear) l0=$(locks)
logind_test Lock _32
check "lock (palette): detected" nested_wait 5 more_locks "$l0"
check "lock (palette): panel closes" nested_wait 5 panel_down
check "lock (palette): act_clear and diag_clear" cleared_after "$c0" "$d0"
touch "$D/pal_slow_ask"
focus dictate-test-lock
pal_open || bad "lock (palette, asking): open"
pal_type palmarker || bad "lock (palette, asking): type"
a0=$(count 'pal_ask 9') h0=$(count 'pal_ask hung up')
key ctrl+return
nested_wait 5 more 'pal_ask 9' "$a0" || bad "lock (palette, asking): asked"
sleep 0.3
logind_test Lock _32
check "lock (palette, asking): panel closes" nested_wait 5 panel_down
check "lock (palette, asking): the AI call hung up" nested_wait 5 more 'pal_ask hung up' "$h0"
rm -f "$D/pal_slow_ask"
settle
focus dictate-test-lock
pal_open || bad "lock (palette, voice): open"
t0=$(count 'start_target palette') c0=$(count cancel) s0=$(count stop) v0=$(pal_renders Voice)
echo 1 >"$KEYS"
press alt+e
nested_wait 3 more 'start_target palette' "$t0" || bad "lock (palette, voice): recording for the palette"
nested_wait 3 more_pal_renders Voice "$v0" || bad "lock (palette, voice): recording view"
logind_test Lock _32
check "lock (palette, voice): panel closes" nested_wait 5 panel_down
check "lock (palette, voice): recording cancelled" nested_wait 5 more cancel "$c0"
echo 0 >"$KEYS"
sleep 0.4
check "lock (palette, voice): releasing E sends no stop" test "$(count stop)" -eq "$s0"
# Gegenprobe: das nächste Diktat gehört wieder dem Fenster
nested_wait 5 overlay_down
focus "$PAL_APP" || bad "lock (palette, voice): focus the window"
w0=$(ui_has 'recording for the window') n0=$(tlen pal)
echo 1 >"$KEYS"
press alt+e
check "lock (palette, voice): the next dictation is for the window" nested_wait 3 more_ui 'recording for the window$' "$w0"
echo 0 >"$KEYS"
check "lock (palette, voice): its text reaches the window" nested_wait 8 pal_len_is $((n0 + FINAL_LEN))
nested_wait 5 overlay_down

check "lock: reasons named in the log" bash -c "grep -q 'session locked (locked hint)' '$UI_LOG' && grep -q 'session locked (lock signal)' '$UI_LOG' && grep -q 'session locked (prepare for sleep)' '$UI_LOG'"

# --- 29. Mitten in der Arbeit: Aufruf abgebrochen, Sitzung gelöscht -------------------------------------------------
touch "$D/slow_run"
open_panel dictate-test-lock || bad "lock (work): panel open"
settle
k0=$(acts act_cancel) c0=$(acts act_clear) d0=$(acts diag_clear)
key e
sleep 0.6
logind_test Lock _32
check "lock (work): panel closes" nested_wait 5 panel_down
check "lock (work): running action cancelled" nested_wait 5 more_acts act_cancel "$k0"
check "lock (work): act_clear and diag_clear" cleared_after "$c0" "$d0"
rm -f "$D/slow_run"

# --- 30. Geschlossenes Panel: trotzdem löschen (wie früher on_lock in actions.lua)------------------------------------------
c0=$(acts act_clear) d0=$(acts diag_clear)
logind_test Lock _32
check "lock (closed panel): act_clear and diag_clear" cleared_after "$c0" "$d0"

# --- 31. Gegenproben: fremde Sitzung, Entsperren, nach dem Schlaf --------------------------------------------------
open_panel dictate-test-lock || bad "lock (negative): panel open"
c0=$(acts act_clear) d0=$(acts diag_clear) l0=$(locks)
logind_test Lock _33
logind_test SetLockedHint _33 true
logind_test SetLockedHint _32 false
logind_test Sleep false
check "lock: foreign Lock sent" foreign_emit /org/freedesktop/login1/session/_32 org.freedesktop.login1.Session.Lock
check "lock: foreign LockedHint sent" foreign_emit /org/freedesktop/login1/session/_32 org.freedesktop.DBus.Properties.PropertiesChanged "'org.freedesktop.login1.Session'" "{'LockedHint': <true>}" "@as []"
check "lock: foreign PrepareForSleep sent" foreign_emit /org/freedesktop/login1 org.freedesktop.login1.Manager.PrepareForSleep true
sleep 0.8
check "lock: other session, unlock, wake-up and a foreign sender do not count" test "$(locks)" -eq "$l0"
check "lock: panel stays open on the negatives" panel_up
check "lock: nothing cleared on the negatives" test "$(acts act_clear)" -eq "$c0" -a "$(acts diag_clear)" -eq "$d0"
logind_test SetLockedHint _33 false
key escape
nested_wait 5 panel_down

# --- 31b. Gesperrt (LockedHint gesetzt): Mod+I öffnet nicht, nach dem Entsperren wieder ---------------------------
logind_test SetLockedHint _32 true
sleep 0.3
g0=$(acts act_grab)
focus dictate-test-lock
key alt+i
check "lock: Mod+I refused while locked" nested_wait 5 grep -q 'session is locked, not opening' "$UI_LOG"
check "lock: refusal shown in the overlay" nested_wait 3 overlay_up
sleep 0.5
check "lock: no panel and no grab while locked" test -z "$(panel_json)" -a "$(acts act_grab)" -eq "$g0"
dp0=$(acts diag_plan)
sleep 3.2 # der Hinweis von Mod+I ist abgelaufen
diag_key
check "lock: Mod+Shift+I refused while locked" nested_wait 5 grep -q 'diagnose: session is locked, not opening' "$UI_LOG"
check "lock: diagnose refusal shown in the overlay" nested_wait 3 overlay_up
sleep 0.5
check "lock: no diagnosis while locked" test -z "$(panel_json)" -a "$(acts diag_plan)" -eq "$dp0"
pr0=$(count 'pal_resolve 0')
sleep 3.2 # der Hinweis von Mod+Shift+I ist abgelaufen
nested_wait 5 overlay_down
key alt+space
check "lock: Mod+Alt+Space refused while locked" nested_wait 5 grep -q 'palette: session is locked, not opening' "$UI_LOG"
check "lock: palette refusal shown in the overlay" nested_wait 3 overlay_up
sleep 0.5
check "lock: no palette while locked" test -z "$(panel_json)" -a "$(count 'pal_resolve 0')" -eq "$pr0"
logind_test SetLockedHint _32 false
sleep 0.3
check "lock: Mod+I opens again after unlocking" open_panel dictate-test-lock
key escape
nested_wait 5 panel_down
check "lock: Mod+Shift+I opens again after unlocking" diag_open
key escape
nested_wait 5 panel_down
check "lock: Mod+Alt+Space opens again after unlocking" pal_open
pal_close

# --- 32. Neustart: unbekannte XDG_SESSION_ID fällt auf "auto" zurück; LockedHint schon beim Start gesetzt -----------
kill -TERM "$UI_PID"
nested_wait 5 bash -c "! kill -0 $UI_PID 2>/dev/null"
cp "$UI_LOG" "$D/ui-actions.log"
: >"$LOGIND_CALLS"
UI_SESSION_ID=9 start_ui || bad "lock: restart dictate-ui"
check "lock: unknown XDG_SESSION_ID falls back to auto" nested_wait 5 bash -c "[ \"\$(sort '$LOGIND_CALLS' | tr '\n' ' ')\" = '9 auto ' ]"
check "lock: watching the own session via auto" nested_wait 5 grep -q 'session/_32 (from "auto")' "$UI_LOG"
c0=$(acts act_clear) d0=$(acts diag_clear)
logind_test Lock _32
check "lock: detected after the fallback" cleared_after "$c0" "$d0"

# veraltete XDG_SESSION_ID einer anderen, bekannten Sitzung: beide werden beobachtet, mit Warnung
kill -TERM "$UI_PID"
nested_wait 5 bash -c "! kill -0 $UI_PID 2>/dev/null"
cp "$UI_LOG" "$D/ui-fallback.log"
UI_SESSION_ID=3 start_ui || bad "lock: restart dictate-ui with another session"
check "lock: both sessions watched" nested_wait 5 bash -c "grep -q 'session/_33 (from \"3\")' '$UI_LOG' && grep -q 'session/_32 (from \"auto\")' '$UI_LOG'"
check "lock: differing sessions warned" grep -q 'lock: logind sessions differ' "$UI_LOG"
for sess in _33 _32; do
    c0=$(acts act_clear) d0=$(acts diag_clear)
    logind_test Lock "$sess"
    check "lock: session $sess counts" cleared_after "$c0" "$d0"
done
# hängender Hinweis der veralteten Sitzung: löscht, sperrt Mod+I aber nicht (nur die Sitzung hinter "auto" sperrt)
c0=$(acts act_clear) d0=$(acts diag_clear)
logind_test SetLockedHint _33 true
check "lock: stale session hint clears" cleared_after "$c0" "$d0"
check "lock: stale session hint does not block Mod+I" open_panel dictate-test-lock
key escape
nested_wait 5 panel_down
logind_test SetLockedHint _33 false

logind_test SetLockedHint _32 true
kill -TERM "$UI_PID"
nested_wait 5 bash -c "! kill -0 $UI_PID 2>/dev/null"
cp "$UI_LOG" "$D/ui-both.log"
c0=$(acts act_clear) d0=$(acts diag_clear)
start_ui || bad "lock: restart dictate-ui while locked"
check "lock: already locked at start clears" cleared_after "$c0" "$d0"
check "lock: reason locked at start" nested_wait 5 grep -q 'session locked (locked at start)' "$UI_LOG"
logind_test SetLockedHint _32 false

# --- 33. Erkennung aus (kein System-Bus): Hinweis im Panel, Panel bleibt bedienbar ---------------------------------
kill -TERM "$UI_PID"
nested_wait 5 bash -c "! kill -0 $UI_PID 2>/dev/null"
cp "$UI_LOG" "$D/ui-locked-start.log"
UI_SYSBUS="unix:path=$NESTED_RT/no-such-bus" start_ui || bad "lock: restart dictate-ui without a bus"
check "lock: missing bus warned" nested_wait 5 grep -q 'lock: no system bus' "$UI_LOG"
check "lock off: panel still opens" open_panel dictate-test-lock
check "lock off: notice in the panel" nested_wait 3 grep -q 'panel: lock detection off notice shown' "$UI_LOG"
check "lock off: tried again after 2 s" nested_wait 6 bash -c "(( \$(grep -c 'lock: no system bus' '$UI_LOG') >= 2 ))"
key escape
nested_wait 5 panel_down
check "lock off: diagnosis still opens" diag_open
check "lock off: notice in the diagnosis" nested_wait 3 grep -q 'diagnose: lock detection off notice shown' "$UI_LOG"
nested_shot "$D/diag-lock-off.png"
key escape
nested_wait 5 panel_down
check "lock off: palette still opens" pal_open
check "lock off: notice in the palette" nested_wait 3 grep -q 'palette: lock detection off notice shown' "$UI_LOG"
pal_close
