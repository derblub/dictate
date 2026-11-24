# Copyright (C) 2026 Daniel Kurdoghlian
# Pushing Pixels, Vienna <https://pushingpixels.at>
# SPDX-License-Identifier: GPL-3.0-or-later
# Prüfungen der Textaktionen von dictate-ui (Block W3), eingebunden von tests/wayland/run.sh nach output.sh
# (verschachteltes niri, Mock-Daemon mit `act_*`). Markierung in einem GTK-Feld (PRIMARY), Mod+I (verschachtelt
# Alt+I) öffnet das Panel `dictate-panel` schwebend und fokussiert, Aktion mit gestreamten Stücken, Copy in CLIPBOARD
# (ohne Hinweis, Entscheidung 12), Höhe wächst über niri (Entscheidung 13), Replace im Zielfenster, verweigert ohne
# Zielfenster und im Terminal, Schließen über Esc, zweites Mod+I und Fokusverlust (jedes Mal `act_clear`), Stop
# mitten in der Arbeit, Diktat bei fokussiertem Panel verweigert. Nur Testtexte; ins Log von dictate-ui darf keiner davon gelangen (Prüfung am Ende von run.sh).

SEL='Markierter Text für die Textaktionen: äöü ß „ok“'
OLD='Alter Text zum Ersetzen'
REPLACED='Ersetzter Text aus dem Panel'
PANEL=dictate-panel

cp "$UI_LOG" "$D/ui-output.log"
echo '{"typing": {"output": "paste"}, "actions": {"terminal_classes": ["Alacritty", "XTerm", "xterm", "dictate-test-term"]}}' >"$D/config.json"
start_ui || bad "actions: start dictate-ui"
nested_wait 5 grep -q 'connected to the daemon' "$UI_LOG" || bad "actions: ui connects to the daemon"
nested_wait 5 grep -q 'text output ready' "$UI_LOG"

panel_json() { nmsg -j windows | python3 -c 'import json,sys
w=[w for w in json.load(sys.stdin) if w.get("app_id")=="dictate-panel"]
print(json.dumps(w[0]) if w else "")'; }
panel_up() { [[ -n $(panel_json) ]]; }
panel_down() { ! panel_up; }
panel_field() { panel_json | python3 -c 'import json,sys; w=json.loads(sys.stdin.read() or "null"); print(eval(sys.argv[1]) if w else "")' "$1"; }
# Panel ganz auf seinem Ausgang (Oberkante ≥ 0, Unterkante der Kachel ≤ logische Höhe; Befund Block C, W4): Ausgang über
# den Arbeitsbereich des Fensters
panel_inside() {
    local w o s
    w=$(panel_json) && [[ -n $w ]] || return 1
    o=$(nmsg -j outputs) && s=$(nmsg -j workspaces) || return 1
    python3 -c 'import json,sys
w, outs, wss = (json.loads(a) for a in sys.argv[1:4])
out = next(x["output"] for x in wss if x["id"] == w["workspace_id"])
h = outs[out]["logical"]["height"]
top = w["layout"]["tile_pos_in_workspace_view"][1]
bottom = top + w["layout"]["tile_size"][1]
print(f"info panel top {top:.0f} bottom {bottom:.0f} output {h}")
sys.exit(0 if top >= 0 and bottom <= h + 0.5 else 1)' "$w" "$o" "$s"
}
panel_height() { panel_field 'w["layout"]["window_size"][1]'; }
acts() { grep -c " $1" "$MOCK_LOG"; }
more_acts() { (($(acts "$1") > $2)); }
key() { nkeys "$1" && sleep 0.05; }
# Totzeit des Panels (0,3 s) nach jedem Zustandswechsel abwarten
settle() { sleep 0.45; }
open_panel() { # APP: Fenster fokussieren, Mod+I, warten bis das Panel fokussiert ist und gegriffen hat
    local g
    g=$(acts act_grab)
    focus "$1" || return 1
    key alt+i || return 1
    nested_wait 5 panel_up || return 1
    nested_wait 5 more_acts act_grab "$g" || return 1
    nested_wait 3 panel_focused
}
panel_focused() { [[ $(nmsg -j focused-window | python3 -c 'import json,sys; print((json.load(sys.stdin) or {}).get("app_id"))') == "$PANEL" ]]; }
panel_floats() { [[ $(panel_field 'w["is_floating"]') == True ]]; }
grab_exact() { [[ "$(cat "$D/grab.txt")" == "$SEL" ]]; }
no_hint() { ! clip_types | grep -q x-kde-passwordManagerHint; }
gone_closed() { ! has_win dictate-test-gone; }
# CLIPBOARD (Text) gleich dem letzten Ergebnis des Mocks (byte-genau)
clip_is_result() {
    clip_get 'text/plain;charset=utf-8' >"$D/copy.bin" || return 1
    cmp -s "$D/copy.bin" "$D/result.txt"
}
run_done() { nested_wait "${2:-8}" bash -c "(( \$(grep -cE 'act_run [0-9]+: (result|error|cancelled|ended)' '$UI_LOG') > $1 ))"; }
runs() { grep -cE 'act_run [0-9]+: (result|error|cancelled|ended)' "$UI_LOG"; }
# Ergebnis im Panel gezeichnet: `run_done` sieht nur die Zeile des Arbeiters; der Hauptthread wechselt erst danach in
# „Result“ (unter Last gemessen 924 ms später, Lauf 20261122-145607), und ein Return davor verfällt in „Working“ bzw.
# der Totzeit. Darum vor jedem Return auf ein Ergebnis auf eine neue Lage des Knopfs `copy` warten (nur in „Result“)
copies() { grep -c 'panel: layout copy ' "$UI_LOG"; }
result_shown() { # R0 L0: Lauf nach R0 fertig und ein Knopf `copy` nach L0 gezeichnet
    run_done "$1" && nested_wait 5 bash -c "(( \$(grep -c 'panel: layout copy ' '$UI_LOG') > $2 ))"
}
clip_get() { WAYLAND_DISPLAY="$NESTED_NIRI_WL" "$CLIP" get "$1" 2>/dev/null; }
clip_types() { WAYLAND_DISPLAY="$NESTED_NIRI_WL" "$CLIP" types 2>/dev/null; }
# Ereignisstrom des verschachtelten niri mitschreiben (jede Größe des Panels, auch kurze); nur der eigene Socket
events_start() { # DATEI
    [[ $NESTED_NIRI_SOCKET == "$NESTED_RT"/* ]] || return 70
    nested_spawn "$1" env NIRI_SOCKET="$NESTED_NIRI_SOCKET" niri msg -j event-stream
    EVENTS_PID=$NESTED_LAST_PID
}
events_stop() { kill -TERM -- "-$EVENTS_PID" 2>/dev/null; }
# Alle Höhen des Panels aus dem Ereignisstrom, der Reihe nach
panel_heights() { # DATEI
    python3 - "$1" <<'PY'
import json, sys
ids, hs = set(), []
for line in open(sys.argv[1]):
    try:
        e = json.loads(line)
    except ValueError:
        continue
    wins = e.get("WindowsChanged", {}).get("windows", [])
    if "WindowOpenedOrChanged" in e:
        wins = [e["WindowOpenedOrChanged"]["window"]]
    for w in wins:
        if w.get("app_id") == "dictate-panel":
            ids.add(w["id"])
            hs.append(w["layout"]["window_size"][1])
    for wid, lay in e.get("WindowLayoutsChanged", {}).get("changes", []):
        if wid in ids:
            hs.append(lay["window_size"][1])
print(" ".join(map(str, hs)))
PY
}
# Mitte einer Bedienfläche in logischen Koordinaten: Lage im Fenster aus dem Debug-Log (`panel: layout`), Lage des
# Fensters aus niri
# Lage und Größe des Panels stehen still (zwei Lesungen im Abstand von 0,3 s gleich): das Hochschieben nach dem Wachsen
# läuft im Resizer-Thread über mehrere IPC-Anfragen; ein vorher berechnetes Klickziel traf unter Last daneben (✕ bei
# y=254 statt 155, volle Suite 21.11. bei Last 25)
panel_geom() { panel_field '(w["layout"]["tile_pos_in_workspace_view"], w["layout"]["tile_size"])'; }
panel_still() {
    local a b
    a=$(panel_geom) && [[ -n $a ]] || return 1
    sleep 0.3
    b=$(panel_geom)
    [[ $a == "$b" ]]
}
# Panel liegt auf dem Ausgang und steht still: zwischen neuer Höhe und Hochschieben stand die Lage unter Last rund 0,5 s
# lang still (also reicht `panel_still` allein nicht), ragte dabei aber über (222 + 760 > 899)
panel_settled() { panel_inside && panel_still; }
item_xy() { # ID
    local l
    nested_wait 5 panel_settled || return 1
    l=$(grep "panel: layout $1 " "$UI_LOG" | tail -n 1)
    [[ -n $l ]] || return 1
    panel_json | python3 -c 'import json,sys
w = json.loads(sys.stdin.read())["layout"]
x, y, bw, bh = (float(v) for v in sys.argv[1].split()[-4:])
tx, ty = w["tile_pos_in_workspace_view"]
ox, oy = w["window_offset_in_tile"]
print(round(tx + ox + x + bw / 2), round(ty + oy + y + bh / 2))' "$l"
}
click_item() { # ID: Maus dorthin, klicken
    local xy
    xy=$(item_xy "$1") || return 1
    echo "info click $1 at $xy"
    # shellcheck disable=SC2086
    nclick $xy
}

# --- 21. Markierung (PRIMARY) → Mod+I öffnet das Panel schwebend und fokussiert -----------------------------------
nested_spawn sel python3 "$HERE/target.py" text dictate-test-sel "$SEL"
nested_wait 10 has_win dictate-test-sel || bad "actions: selection window"
nested_wait 5 grep -q '^selected' "$NESTED_DIR/sel.log"
check "actions: Mod+I opens the panel, focused" open_panel dictate-test-sel
check "actions: panel floats" panel_floats
check "actions: grab sends the selection with the app_id" grep -q " act_grab ${#SEL} dictate-test-sel\$" "$MOCK_LOG"
check "actions: daemon got PRIMARY exactly" grab_exact
nmsg -j windows >"$D/windows-w3.json"
h_select=$(panel_height)
w_select=$(panel_field 'w["layout"]["window_size"][0]')
echo "info panel select: ${w_select}x${h_select}"

# --- 21b. Diktat bei fokussiertem Panel wird verweigert: „e“ getippt wäre die Aktionstaste Explain ----------------
r0=$(acts "act_run")
o0=$(grep -c 'text actions panel has the focus' "$UI_LOG")
send_final "e"
check "actions: dictation into the focused panel is refused" nested_wait 5 bash -c "(( \$(grep -c 'text actions panel has the focus' '$UI_LOG') > $o0 ))"
sleep 0.4
check "actions: refused dictation runs no action" test "$(acts act_run)" -eq "$r0"
check "actions: panel stays open after the refused dictation" panel_up
check "actions: panel keeps the focus" panel_focused

# --- 22. Aktion mit gestreamten Stücken, Panel wächst über niri ------------------------------------------------------
settle
r0=$(runs) l0=$(copies)
key e
check "actions: explain runs" result_shown "$r0" "$l0"
check "actions: answer arrived in several deltas" bash -c "grep -E 'act_run [0-9]+: result' '$UI_LOG' | tail -n 1 | grep -qE ' ([2-9]|[1-9][0-9]+) deltas'"
sleep 0.5
h_result=$(panel_height)
echo "info panel result: $(panel_field 'w["layout"]["window_size"][0]')x${h_result}"
check "actions: panel grew with the answer (${h_select} → ${h_result})" test "${h_result:-0}" -gt "${h_select:-0}"
check "actions: height set through niri" grep -q 'panel: height' "$UI_LOG"
check "actions: width stays from the window rule" test "$(panel_field 'w["layout"]["window_size"][0]')" = "$w_select"
check "actions: panel stays on the output" nested_wait 3 panel_inside
nested_shot "$D/panel-result.png"

# --- 23. Copy: Ergebnis in CLIPBOARD, ohne Passwort-Hinweis; Panel zu, Sitzung gelöscht ------------------------------
settle
c0=$(acts act_clear)
key return
check "actions: copy fetches the result" nested_wait 5 grep -q ' act_result$' "$MOCK_LOG"
check "actions: copy closes the panel" nested_wait 5 panel_down
check "actions: closing clears the session" nested_wait 5 more_acts act_clear "$c0"
check "actions: CLIPBOARD holds the answer" clip_is_result
check "actions: copy carries no password hint (decision 12)" no_hint
clip_types >"$D/copy-types.txt"
check "actions: copied text served as text types" grep -q 'text/plain;charset=utf-8' "$D/copy-types.txt"

# --- 24. Replace im Zielfenster (Fokus kehrt nach dem Schließen zurück) ----------------------------------------------
nested_spawn rep python3 "$HERE/target.py" text dictate-test-rep "$OLD"
nested_wait 10 has_win dictate-test-rep || bad "actions: replace window"
nested_wait 5 grep -q '^selected' "$NESTED_DIR/rep.log"
# Wieder geöffnet nach der hohen Antwort: erscheint gleich in der Größe der Auswahl, nicht erst in der alten Höhe
events_start events-reopen
sleep 0.3
check "actions: panel for replace" open_panel dictate-test-rep
sleep 0.5
events_stop
heights=$(panel_heights "$D/events-reopen.log")
echo "info panel heights on reopen: $heights"
check "actions: reopened panel never shows the last height (${h_result}; seen: ${heights})" python3 -c 'import sys; h=[int(x) for x in sys.argv[1].split()]; sys.exit(0 if h and max(h) < int(sys.argv[2]) else 1)' "$heights" "${h_result:-0}"
settle
r0=$(runs) l0=$(copies)
key r
check "actions: rephrase runs" result_shown "$r0" "$l0"
settle
key return
check "actions: replace writes into the target window" nested_wait 8 text_is rep "$REPLACED"
check "actions: replace went through the output queue" grep -q 'text actions: replace queued as output job' "$UI_LOG"

# --- 25. Replace verweigert: Zielfenster weg (Fokus kehrt nicht zurück), Terminal ------------------------------------
nested_spawn gone python3 "$HERE/target.py" text dictate-test-gone "$OLD"
GONE_PID=$NESTED_LAST_PID
nested_wait 10 has_win dictate-test-gone || bad "actions: window that goes away"
nested_wait 5 grep -q '^selected' "$NESTED_DIR/gone.log"
check "actions: panel for the vanishing window" open_panel dictate-test-gone
settle
r0=$(runs) l0=$(copies)
key r
result_shown "$r0" "$l0"
kill -TERM "$GONE_PID" 2>/dev/null
nested_wait 5 gone_closed
rep_before=$(text_of rep)
settle
key return
check "actions: no replace without the target window" nested_wait 6 grep -q 'did not get the focus back' "$UI_LOG"
check "actions: other windows untouched" text_is rep "$rep_before"

nested_spawn term python3 "$HERE/target.py" text dictate-test-term "$OLD"
nested_wait 10 has_win dictate-test-term || bad "actions: terminal-like window"
nested_wait 5 grep -q '^selected' "$NESTED_DIR/term.log"
check "actions: panel in a terminal" open_panel dictate-test-term
settle
r0=$(runs) l0=$(copies)
key r
result_shown "$r0" "$l0"
q0=$(grep -c 'replace queued' "$UI_LOG")
res0=$(acts act_result)
settle
key return
check "actions: in a terminal Enter copies instead of replacing" nested_wait 5 more_acts act_result "$res0"
check "actions: nothing queued for the terminal" test "$(grep -c 'replace queued' "$UI_LOG")" -eq "$q0"
check "actions: terminal text unchanged" text_is term "$OLD"

# --- 26. Schließen: Esc, zweites Mod+I, Fokusverlust; Stop mitten in der Arbeit -------------------------------------
c0=$(acts act_clear)
check "actions: panel again" open_panel dictate-test-term
settle
key escape
check "actions: Esc closes" nested_wait 5 panel_down
check "actions: Esc clears" nested_wait 5 more_acts act_clear "$c0"
c0=$(acts act_clear)
open_panel dictate-test-term
key alt+i
check "actions: second Mod+I closes" nested_wait 5 panel_down
check "actions: second Mod+I clears" nested_wait 5 more_acts act_clear "$c0"
c0=$(acts act_clear)
open_panel dictate-test-term
focus dictate-test-rep
check "actions: focus loss closes" nested_wait 5 panel_down
check "actions: focus loss clears" nested_wait 5 more_acts act_clear "$c0"
touch "$D/slow_run"
open_panel dictate-test-term
settle
k0=$(acts act_cancel)
key e
sleep 0.6
key escape
check "actions: Esc during work cancels the call" nested_wait 5 more_acts act_cancel "$k0"
check "actions: panel stays open after Stop" panel_up
c0=$(acts act_clear)
settle
key escape
nested_wait 5 panel_down
rm -f "$D/slow_run"

# --- 26b. Maus: Klick auf eine Zeile startet die Aktion, Klick auf ✕ schließt (Hover baut nicht neu auf) ----------
open_panel dictate-test-term || bad "actions: panel for the mouse"
settle
r0=$(runs)
check "actions: click on the row Explain" click_item action:explain
check "actions: explain ran after the click" run_done "$r0"
settle
c0=$(acts act_clear)
click_item close
check "actions: click on ✕ closes" nested_wait 5 panel_down
check "actions: ✕ clears" nested_wait 5 more_acts act_clear "$c0"
check "actions: no panel left open" panel_down

# --- 26c. Panel öffnet mitten im Tippen: kein Zeichen mehr (Zustand des Hauptthreads, nicht nur die app_id von niri) --
set_config '{"typing": {"output": "type", "delay_ms": 40, "keyboard_only_app_ids": ["dictate-test-typ"]}, "actions": {"terminal_classes": ["Alacritty", "XTerm", "xterm", "dictate-test-term"]}}' || bad "actions: config reload (slow typing)"
nested_spawn typ python3 "$HERE/target.py" text dictate-test-typ "x"
nested_wait 10 has_win dictate-test-typ || bad "actions: typing window"
nested_wait 5 grep -q '^selected' "$NESTED_DIR/typ.log"
focus dictate-test-typ
p0=$(grep -c 'text actions panel has the focus' "$UI_LOG")
d0=$(done_count)
send_final "$(python3 -c 'print("abcdefghij" * 15, end="")')"
typed10() { (($(tlen typ) >= 10)); }
nested_wait 5 typed10
key alt+i
check "actions: typing stops when the panel opens" nested_wait 6 bash -c "(( \$(grep -c 'text actions panel has the focus' '$UI_LOG') > $p0 ))"
wait_done "$d0"
# schon gesendete Tasten verarbeitet das Fenster noch: erst ruhig werden lassen, dann darf nichts mehr dazukommen
sleep 0.6
l1=$(tlen typ)
sleep 0.8
check "actions: nothing typed after the panel opened ($l1 of 150)" test "$l1" -lt 150 -a "$(tlen typ)" -eq "$l1"
nested_wait 5 panel_up
settle
key escape
nested_wait 5 panel_down
set_config '{"typing": {"output": "paste"}, "actions": {"terminal_classes": ["Alacritty", "XTerm", "xterm", "dictate-test-term"]}}' || bad "actions: config reload (paste)"
