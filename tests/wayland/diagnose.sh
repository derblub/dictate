# Copyright (C) 2026 Daniel Kurdoghlian
# Pushing Pixels, Vienna <https://pushingpixels.at>
# SPDX-License-Identifier: GPL-3.0-or-later
# Prüfungen des Diagnose-Panels von dictate-ui (Block W4, PLAN-wayland.md 6e „Abnahme“), eingebunden von
# tests/wayland/run.sh nach actions.sh (nutzt dessen Helfer) und vor lock.sh (die Sperre der Diagnose prüft lock.sh).
# Gegen den Mock-Daemon mit `diag_*` (tests/wayland/mock_daemon.py): Mod+Shift+I (verschachtelt Alt+Shift+I) öffnet
# schwebend, fokussiert und 800 breit, ein zweites schließt; Ablauf Profil → Einträge → Fortschritt → Vorschau (pstore
# als Abschnitt, Feld „What is wrong?“) → Senden → Antwort → Werte → Beleg → Copy report mit `c` (CLIPBOARD byte-genau
# der Bericht, ohne Klartext der Pseudonyme) → Esc (`diag_clear`); Stop durch Auflegen beim Sammeln und Senden,
# Schließen mitten im Sammeln; ein Panel zur Zeit über die Kürzel; Boot-Hinweis (erste Frage nach 10 s, Wiederholen bei
# `busy` und fehlendem Daemon, höchstens 4-mal, Warten während einer Aufnahme, Klick und Kürzel öffnen, Ablauf, danach
# wieder klick-durch). Testtexte des Mocks (`diagmarker`, `geheimhost`, `erika.musterfrau`, Symptom, pstore) dürfen nie
# ins Log von dictate-ui (Prüfung am Ende von run.sh).

SYMPTOM_LEN=38 # „mein geheimhost.lan bootet nicht check“ (nur Tasten, die unter `de` und `us` gleich liegen)

renders() { grep -c "diagnose: rendered $1 in" "$UI_LOG"; }
more_renders() { (($(renders "$1") > $2)); }
panel_width() { panel_field 'w["layout"]["window_size"][0]'; }
boot_asks() { grep -c ' diag_boot_hint$' "$MOCK_LOG"; }
diag_key() { # Mod+Shift+I
    key shift+alt+i
}
diag_open() { # Mod+Shift+I: Panel mit den Profilen, fokussiert
    local p
    p=$(renders Profiles)
    diag_key || return 1
    nested_wait 5 panel_up || return 1
    nested_wait 5 more_renders Profiles "$p" || return 1
    nested_wait 5 grep -q 'panel: layout profile:1 ' "$UI_LOG" || return 1
    nested_wait 3 panel_focused
}
# Profil 1 (Last boot) wählen: warten, bis die Einträge gezeichnet sind (Totzeit danach abgewartet)
diag_items() {
    local i
    i=$(renders Items)
    settle
    key 1
    nested_wait 5 more_renders Items "$i" || return 1
    settle
}
# Profil 1 sammeln: Einträge, Enter, warten bis zur Vorschau
diag_collect() {
    local v
    diag_items || return 1
    v=$(renders Preview)
    key return
    nested_wait "${1:-15}" more_renders Preview "$v"
}
restart_ui() { # NAME: laufende Oberfläche beenden, ihr Log als ui-NAME.log aufheben, neu starten (UI_BOOT_MS wirkt)
    kill -TERM "$UI_PID"
    nested_wait 5 bash -c "! kill -0 $UI_PID 2>/dev/null"
    cp "$UI_LOG" "$D/ui-$1.log"
    start_ui || return 1
    # GTK allein brauchte bei Last 25 schon 6 s bis „ready“ (volle Suite 21.11.); sonst rund 1 s
    nested_wait 15 grep -q 'connected to the daemon' "$UI_LOG"
}
since() { # NS: Zeilen des Mock-Logs ab diesem Zeitpunkt
    awk -v t="$1" '$1 >= t' "$MOCK_LOG"
}

# Frische Oberfläche mit den echten Zeiten des Boot-Hinweises (die erste Frage prüft D8)
rm -f "$D/boot_hint" "$D/slow_diag" "$D/big_diag" "$D/fail_collect"
t_boot=$(now_ns)
restart_ui actions-panel || bad "diagnose: restart dictate-ui"
focus dictate-test-target || bad "diagnose: focus the target window"

# --- D1. Mod+Shift+I öffnet schwebend, fokussiert, 800 breit; ein zweites schließt ------------------------------------
check "diagnose: Mod+Shift+I opens the panel, focused" diag_open
check "diagnose: panel floats" panel_floats
check "diagnose: 800 wide (text actions 720)" test "$(panel_width)" = 800
check "diagnose: no lock-off notice while lock detection works" bash -c "! grep -q 'diagnose: lock detection off notice shown' '$UI_LOG'"
nested_shot "$D/diag-profiles.png"
cl0=$(acts diag_clear)
diag_key
check "diagnose: second Mod+Shift+I closes" nested_wait 5 panel_down
check "diagnose: closing clears the diagnosis" nested_wait 5 more_acts diag_clear "$cl0"

# --- D2. Ablauf bis zur Antwort, pstore als Abschnitt, Feld „What is wrong?“ ---------------------------------------------
diag_open || bad "diagnose: open again"
check "flow: collect to the preview" diag_collect
check "flow: pstore (third probe of the profile) is a section of the preview" nested_wait 3 grep -q 'panel: layout sec:S3 ' "$UI_LOG"
settle
nested_shot "$D/diag-preview.png"
c0=$(acts diag_copy) s0=$(acts diag_symptom)
key tab
sleep 0.3
nkeys text:mein space text:geheimhost.lan space text:bootet space text:nicht space text:check
sleep 0.3
check "field: typed keys stay in the field (no copy)" test "$(acts diag_copy)" -eq "$c0"
key return
check "field: Enter applies the symptom" nested_wait 5 more_acts diag_symptom "$s0"
check "field: symptom has its length" grep -q " diag_symptom $SYMPTOM_LEN\$" "$MOCK_LOG"
settle
# Tab ins gefüllte Feld markiert nichts: eine Markierung böte das ungefilterte Symptom als PRIMARY an (Durchsicht W4, F1)
nested_spawn primary "$CLIP" watch-primary
PRIMARY_PID=$NESTED_LAST_PID
nested_wait 5 grep -q 'primary #1' "$NESTED_DIR/primary.log" || bad "field: PRIMARY watcher"
p0=$(grep -c 'primary #' "$NESTED_DIR/primary.log")
key tab
sleep 0.5
key escape
sleep 0.5
check "field: Tab into the filled field leaves PRIMARY alone" test "$(grep -c 'primary #' "$NESTED_DIR/primary.log")" -eq "$p0"
check "field: Esc leaves the field, the panel stays" panel_up
kill -TERM -- "-$PRIMARY_PID" 2>/dev/null
settle
# Abschnitt weglassen und wieder aufnehmen (Space; die ganze Abwahl geht an den Daemon)
d0=$(acts diag_deselected)
key down
settle
key space
check "flow: space leaves a section out" nested_wait 5 more_acts diag_deselected "$d0"
settle
key space
nested_wait 5 more_acts diag_deselected "$((d0 + 1))"
settle
a0=$(renders Answer)
key return
check "flow: send and answer" nested_wait 10 more_renders Answer "$a0"
settle
key p
settle
nested_shot "$D/diag-answer-values.png"

# --- D3. Copy report mit `c`, nachdem Klartext im Speicher liegt: CLIPBOARD ist der Bericht des Daemons --------------------
v0=$(renders Preview)
check "copy: evidence opens the preview" click_item ev:1:2
nested_wait 5 more_renders Preview "$v0"
settle
c0=$(acts diag_copy)
rm -f "$D/diag-copy.txt"
key c
check "copy: c asks the daemon for the report" nested_wait 5 more_acts diag_copy "$c0"
check "copy: the call is logged by name only" nested_wait 5 grep -qE 'diag [a-z_]*copy: ok after [0-9]+ ms$' "$UI_LOG"
diag_clip_is_report() {
    clip_get 'text/plain;charset=utf-8' >"$D/diag-clip.bin" || return 1
    cmp -s "$D/diag-clip.bin" "$D/diag-copy.txt"
}
check "copy: CLIPBOARD holds the report byte for byte" nested_wait 5 diag_clip_is_report
check "copy: no plain text of the pseudonyms in CLIPBOARD" bash -c "! grep -q -e geheimhost -e musterfrau '$D/diag-clip.bin' && grep -q host-1 '$D/diag-clip.bin'"
check "copy: panel stays open" panel_up

# --- D3b. Prüfrunde: zurück zur Antwort, Prüfbefehl wählen und ausführen, neue Vorschau (Runde 2) ----------------------------
a1=$(renders Answer)
key backspace
check "check: back to the answer" nested_wait 5 more_renders Answer "$a1"
settle
k0=$(acts diag_check)
check "check: click selects" click_item check:1
settle
v2=$(renders Preview)
# Bei 900 px Höhe wuchs das Panel hier unten aus dem Bildschirm („Run“ bei y=1004, Befund Block C): dictate-ui schiebt
# es um den Überstand hoch, „Run“ ist danach anklickbar
check "check: panel was moved up into the output" nested_wait 3 grep -q 'panel: move up' "$UI_LOG"
check "check: panel fits on the output" nested_wait 3 panel_inside
check "check: click on Run" click_item run
check "check: diag_check" nested_wait 5 more_acts diag_check "$k0"
check "check: new preview of round 2" nested_wait 15 more_renders Preview "$v2"
check "check: round 2 still on the output" nested_wait 3 panel_inside
settle
cl0=$(acts diag_clear)
key escape
check "copy: Esc closes" nested_wait 5 panel_down
check "copy: ... and clears" nested_wait 5 more_acts diag_clear "$cl0"

# --- D4. Stop durch Auflegen: beim Sammeln (zurück zu den Einträgen) und beim Senden (zurück zur Vorschau) ---------------
touch "$D/slow_diag"
diag_open || bad "stop: open"
diag_items
h0=$(acts "diag_collect hung up") k0=$(acts diag_cancel) i0=$(renders Items) p0=$(renders Progress)
key return
nested_wait 5 more_renders Progress "$p0"
sleep 0.5
key escape
check "stop (collect): back to the items" nested_wait 5 more_renders Items "$i0"
check "stop (collect): the daemon saw the hang-up" nested_wait 5 more_acts "diag_collect hung up" "$h0"
check "stop (collect): diag_cancel with the known id" nested_wait 5 more_acts diag_cancel "$k0"
check "stop (collect): panel stays open" panel_up
settle
v0=$(renders Preview)
key return
check "stop (send): collected again" nested_wait 15 more_renders Preview "$v0"
settle
h0=$(acts "diag_send hung up") v1=$(renders Preview) se0=$(renders Send)
key return
nested_wait 5 more_renders Send "$se0"
sleep 0.5
key escape
check "stop (send): back to the preview" nested_wait 5 more_renders Preview "$v1"
check "stop (send): the daemon saw the hang-up" nested_wait 8 more_acts "diag_send hung up" "$h0"

# --- D5. Schließen mitten im Sammeln: aufgelegt und gelöscht ---------------------------------------------------------------
cl0=$(acts diag_clear)
key escape
nested_wait 5 panel_down
diag_open || bad "close while collecting: open"
diag_items
h0=$(acts "diag_collect hung up") cl0=$(acts diag_clear) p0=$(renders Progress)
key return
nested_wait 5 more_renders Progress "$p0"
sleep 0.4
diag_key
check "close while collecting: panel closes" nested_wait 5 panel_down
check "close while collecting: hung up" nested_wait 5 more_acts "diag_collect hung up" "$h0"
check "close while collecting: diag_clear" nested_wait 5 more_acts diag_clear "$cl0"
rm -f "$D/slow_diag"

# --- D6. Ein Panel zur Zeit über die Kürzel: Mod+Shift+I schließt die Textaktionen nur, Mod+I die Diagnose nur ------------
check "switch: Mod+I opens the text actions" open_panel dictate-test-target
settle
check "switch: text actions 720 wide" test "$(panel_width)" = 720
ac0=$(acts act_clear) dp0=$(acts diag_plan)
diag_key
check "switch: Mod+Shift+I closes the text actions" nested_wait 5 panel_down
check "switch: ... with act_clear" nested_wait 5 more_acts act_clear "$ac0"
sleep 0.5
check "switch: ... and opens no diagnosis" test "$(acts diag_plan)" -eq "$dp0"
focus dictate-test-target
check "switch: Mod+Shift+I opens the diagnosis again" diag_open
cl0=$(acts diag_clear) g0=$(acts act_grab)
key alt+i
check "switch: Mod+I closes the diagnosis" nested_wait 5 panel_down
check "switch: ... with diag_clear" nested_wait 5 more_acts diag_clear "$cl0"
sleep 0.5
check "switch: ... and grabs nothing" test "$(acts act_grab)" -eq "$g0"

# --- D7. Fehleransicht: Sammeln scheitert --------------------------------------------------------------------------------
touch "$D/fail_collect"
focus dictate-test-target
diag_open || bad "error: open"
e0=$(renders Error)
settle
key 1
settle
key return
check "error: drawn" nested_wait 5 more_renders Error "$e0"
settle
key escape
check "error: Esc closes" nested_wait 5 panel_down
rm -f "$D/fail_collect"

# --- D8. Boot-Hinweis mit den echten Zeiten: eine Frage 10 s nach dem Start, „schon geprüft“ beendet das Fragen ------------
# gemessen ab dem Verbinden mit dem Daemon (`subscribe`; GTK steht dann schon, der Zeitgeber läuft ab dem Start)
nested_wait 15 bash -c "awk -v t=$t_boot '\$1 >= t && / diag_boot_hint\$/' '$MOCK_LOG' | grep -q ." || bad "boot hint: first ask"
sub_ns=$(since "$t_boot" | grep ' subscribe$' | head -n 1 | cut -d' ' -f1)
first_ns=$(since "$t_boot" | grep ' diag_boot_hint$' | head -n 1 | cut -d' ' -f1)
dt=$(((${first_ns:-0} - ${sub_ns:-0}) / 1000000))
if ((dt >= 9500 && dt <= 11500)); then ok "boot hint: first ask 10 s after the start ($dt ms after connecting)"; else bad "boot hint: first ask 10 s after the start ($dt ms after connecting, want 9500..11500)"; fi
sleep 1
check "boot hint: one ask when not needed" test "$(since "$t_boot" | grep -c ' diag_boot_hint$')" -eq 1
check "boot hint: nothing shown when not needed" bash -c "! grep -q 'boot hint: shown' '$UI_LOG'"

# --- D9. busy, fehlender Daemon (Verbindung ohne Antwort), dann „ja“: Hinweis, Klick öffnet, nicht durch zum Fenster -------
printf 'busy\ndrop\nshow\n' >"$D/boot_hint"
b0=$(boot_asks)
UI_BOOT_MS=1000,1500,8000 restart_ui diagnose || bad "boot hint: restart"
focus dictate-test-target
check "boot hint: shown after busy and a missing daemon" nested_wait 10 grep -q 'boot hint: shown' "$UI_LOG"
check "boot hint: asked three times" test "$(boot_asks)" -eq $((b0 + 3))
check "boot hint: answers named in the log" bash -c "grep -q 'boot hint: answer busy' '$UI_LOG' && grep -q 'boot hint: answer no daemon' '$UI_LOG'"
check "boot hint: overlay shows it" nested_wait 3 overlay_up
check "boot hint: the overlay takes clicks" nested_wait 3 grep -q 'overlay: input region whole surface' "$UI_LOG"
sleep 0.3
nested_shot "$D/boot-hint.png"
c0=$(clicks) dp0=$(acts diag_plan)
nclick "$OX" "$OY"
check "boot hint: click opens the diagnosis" nested_wait 5 panel_up
check "boot hint: ... with the profiles" nested_wait 5 more_acts diag_plan "$dp0"
check "boot hint: click logged, hint ended" bash -c "grep -q 'boot hint: clicked' '$UI_LOG' && grep -q 'boot hint: ended (diagnosis opened)' '$UI_LOG'"
sleep 0.3
check "boot hint: the click did not reach the window below" test "$(clicks)" -eq "$c0"
nested_wait 3 panel_focused
settle
key escape
nested_wait 5 panel_down
sleep 0.5
check "boot hint: does not come back after the diagnosis" overlay_down
check "boot hint: input region empty again" grep -q 'overlay: input region empty' "$UI_LOG"

# --- D10. Höchstens vier Fragen, wenn der Daemon sammelt ------------------------------------------------------------------
printf 'busy\nbusy\nbusy\nbusy\nbusy\n' >"$D/boot_hint"
b0=$(boot_asks)
UI_BOOT_MS=500,700,3000 restart_ui boot-click || bad "boot hint (busy): restart"
check "boot hint (busy): four asks" nested_wait 8 bash -c "(( \$(grep -c ' diag_boot_hint\$' '$MOCK_LOG') >= $b0 + 4 ))"
sleep 1.5
check "boot hint (busy): no fifth ask" test "$(boot_asks)" -eq $((b0 + 4))
check "boot hint (busy): nothing shown" bash -c "! grep -q 'boot hint: shown' '$UI_LOG'"

# --- D11. „ja“ während einer Aufnahme wartet; danach steht der Hinweis, läuft ab, das Overlay ist wieder klick-durch ------
printf 'show\n' >"$D/boot_hint"
UI_BOOT_MS=2500,1000,4000 restart_ui boot-busy || bad "boot hint (recording): restart"
focus dictate-test-target
s0=$(count start)
echo 1 >"$KEYS"
press alt+e
check "boot hint (recording): listening" nested_wait 3 at_least start $((s0 + 1))
check "boot hint (recording): answer arrives while listening" nested_wait 6 grep -q 'boot hint: answer show' "$UI_LOG"
sleep 0.3
check "boot hint (recording): not shown during the recording" bash -c "! grep -q 'boot hint: shown' '$UI_LOG'"
check "boot hint (recording): overlay stays click-through" click_through
# abbrechen statt loslassen: kein Text für das Klickfenster (Einfügen wartete dort bis zur Frist)
c0=$(count cancel)
press ctrl+alt+e
nested_wait 3 at_least cancel $((c0 + 1))
echo 0 >"$KEYS"
check "boot hint (recording): shown after the recording" nested_wait 5 grep -q 'boot hint: shown' "$UI_LOG"
check "boot hint (recording): ends after its time" nested_wait 7 grep -q 'boot hint: ended$' "$UI_LOG"
check "boot hint (recording): overlay hidden" nested_wait 3 overlay_down
s0=$(count start) c0=$(count cancel)
echo 1 >"$KEYS"
press alt+e
nested_wait 3 at_least start $((s0 + 1))
nested_wait 5 overlay_up
check "boot hint (recording): click passes through again after the hint" click_through
press ctrl+alt+e
nested_wait 3 at_least cancel $((c0 + 1))
echo 0 >"$KEYS"
nested_wait 5 overlay_down

# --- D12. Mod+Shift+I öffnet, solange der Hinweis steht, und beendet ihn --------------------------------------------------
printf 'show\n' >"$D/boot_hint"
UI_BOOT_MS=500,1000,8000 restart_ui boot-recording || bad "boot hint (shortcut): restart"
focus dictate-test-target
check "boot hint (shortcut): shown" nested_wait 6 grep -q 'boot hint: shown' "$UI_LOG"
check "boot hint (shortcut): Mod+Shift+I opens" diag_open
check "boot hint (shortcut): hint ended" grep -q 'boot hint: ended (diagnosis opened)' "$UI_LOG"
key escape
nested_wait 5 panel_down
sleep 0.5
check "boot hint (shortcut): does not come back" overlay_down

# Für lock.sh: Oberfläche mit den echten Zeiten, ohne Antwortdatei
rm -f "$D/boot_hint"
restart_ui boot-shortcut || bad "diagnose: restart dictate-ui for lock.sh"
