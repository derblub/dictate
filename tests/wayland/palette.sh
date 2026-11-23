# Copyright (C) 2026 Daniel Kurdoghlian
# Pushing Pixels, Vienna <https://pushingpixels.at>
# SPDX-License-Identifier: GPL-3.0-or-later
# Prüfungen der Befehlspalette von dictate-ui (Block W5, PLAN-wayland.md 6g, Teil C3), eingebunden von
# tests/wayland/run.sh nach diagnose.sh (nutzt die Helfer aus output.sh, actions.sh und diagnose.sh) und vor lock.sh
# (die Sperre der Palette prüft lock.sh). Gegen den Mock-Daemon mit `pal_*` (tests/wayland/mock_daemon.py, Wörter in
# PAL_WORDS) und die Fake-Programme aus run.sh (`--fake-programs`, jedes schreibt nur Namen und argv nach $FAKE_LOG):
# Mod+Alt+Space (verschachtelt Alt+Space) öffnet schwebend und fokussiert, die Eingabezeile hat den Fokus ohne Markierung
# (kein PRIMARY), Tippen → Kandidaten, Return startet ein Programm über das Fake (`pal_chosen`), Return auf dem Fenster
# fokussiert es, Esc schließt, `confirm` (`reboot`: erst die Bestätigung, dann das Fake `systemctl`), Return vor der
# Antwort wartet auf sie, ausgeschaltete Palette, Ctrl+Return fragt die KI, Esc legt auf, der Vorschlag läuft; Stimme ins
# Panel (Text in der Zeile, kein Ausführen, kein Tippen ins Fenster, Overlay aus; Esc bricht ab, ein spätes `final` wird
# verworfen; danach ein Fensterdiktat), ein Panel zur Zeit nach benutzter Palette (die verborgene Eingabezeile fängt
# keine Tasten der Textaktionen und der Diagnose) und über die Kürzel (jedes schließt das offene andere Panel nur).
# Übergaben (Block D, P14–P18): `ask` (Frage ans Panel der Textaktionen, Antwort sofort, `pal_chosen` danach),
# `explain_clipboard` (CLIPBOARD nur auf diese Absicht, Vorschau, ein gehaltenes Return sendet nichts, Esc auch nicht;
# Passwort-Hinweis: nur er wird gelesen), `text_action` über die KI (PRIMARY mit Vorschau, die vorgewählte Aktion erst auf Return), `diagnose` mit
# Vorbelegung (Profil und Unit; Diagnose aus = keine Wahl). Gegenprobe zu P15: eine gehaltene Taste des Injektors
# wiederholt (P15a); CLIPBOARD aus eigener Quelle (Copy der Textaktionen) kommt aus dem Speicher, ohne Frist (P16b).
# Übrige Absichten (Block D2, Wörter `qq…` des Mocks): Programme, Ordner, Adressen und Websuche mit genau diesem argv
# (P19), System über Kurzläufer samt Grenze 100 % der Lautstärke (P20), confirm für poweroff und ein scheiterndes suspend
# mit Meldung im Overlay (P21); niri-Aktionen im verschachtelten niri: Fokus über Arbeitsflächen, view/previous (P22),
# column_display mit Messung L15 und sichtbarer Wirkung in einer Spalte mit zwei Fenstern (P23), move (P24), die drei
# Schalter auf ein nicht fokussiertes Fenster (P25), close_window mit Bestätigung und ohne Ersatzziel (P26), Screenshots
# ohne Panel im Bild, nur in die Zwischenablage des verschachtelten niri (P27). `logout` beendet niri: tests/wayland/logout.sh.
# Testtexte mit `palmarker` (Eingabe, Anzeige, Fenstertitel) dürfen nie ins Log von dictate-ui (Prüfung am Ende von
# run.sh).

PAL_APP=dictate-test-pal
FINAL_LEN=47 # FINAL des Mocks („Fertiger Testsatz aus dem verschachtelten niri.“)
LATE_LEN=33  # LATE des Mocks („Später Testsatz nach dem Abbruch.“)
FINAL='Fertiger Testsatz aus dem verschachtelten niri.'

more() { (($(count "$1") > $2)); }
pal_renders() { grep -c "palette: rendered $1 " "$UI_LOG"; }
more_pal_renders() { (($(pal_renders "$1") > $2)); }
ui_has() { grep -c -- "$1" "$UI_LOG"; }
more_ui() { (($(ui_has "$1") > $2)); }
fakes() { grep -c -- "^$1" "$FAKE_LOG"; }
more_fakes() { (($(fakes "$1") > $2)); }
# Mod+Alt+Space: Palette schwebend offen und fokussiert, die Antwort auf die leere Eingabe ist da (Totzeit abgewartet)
pal_open() {
    local r
    r=$(count 'pal_resolve 0')
    key alt+space || return 1
    nested_wait 5 panel_up || return 1
    nested_wait 3 panel_focused || return 1
    nested_wait 5 more 'pal_resolve 0' "$r" || return 1
    settle
}
pal_type() { # WORT (nur Tasten, die unter `de` und `us` gleich liegen): warten, bis der Mock die ganze Eingabe hat
    local r
    r=$(count "pal_resolve ${#1}")
    key "text:$1" || return 1
    nested_wait 5 more "pal_resolve ${#1}" "$r" || return 1
    settle
}
pal_window_focused() { [[ $(focused_app) == "$PAL_APP" ]]; }
pal_len_is() { (($(tlen pal) == $1)); }
pal_ends_with_final() { [[ $(text_of pal) == *"$FINAL" ]]; }
pal_close() { # Esc: zu, Totzeit abgewartet
    key escape
    nested_wait 5 panel_down && settle
}

# Übrige Absichten (Block D2): Zählen als ganze, feste Zeile (die argv tragen Regex-Zeichen); Basis und Prüfung mit demselben Muster
fake_n() { grep -cxF -- "$1" "$FAKE_LOG"; }
more_fake_n() { (($(fake_n "$1") > $2)); }
pal_chosen_n() { count "pal_chosen $1"; }
pal_logged() { grep -c "palette: $1\$" "$UI_LOG"; }
more_pal_logged() { (($(pal_logged "$1") > $2)); }
# WORT [confirm]: öffnen, tippen, Return (bei confirm erst die Bestätigung, dann ein zweites Return), Panel zu
pal_run() {
    local k
    pal_open || return 1
    pal_type "$1" || return 1
    if [[ ${2:-} == confirm ]]; then
        k=$(ui_has 'panel: layout confirm ')
        key return
        nested_wait 5 more_ui 'panel: layout confirm ' "$k" || return 1
        settle
        [[ -n ${PAL_BEFORE_CONFIRM:-} ]] && { "$PAL_BEFORE_CONFIRM" || return 1; }
    fi
    key return
    nested_wait 5 panel_down
}
# WORT ABSICHT ZEILE [confirm]: die Fake-Zeile kommt genau einmal dazu, pal_chosen und „done“ im Log
pal_expect() {
    local word=$1 intent=$2 line=$3 f0 c0 d0
    f0=$(fake_n "$line") c0=$(pal_chosen_n "$intent") d0=$(pal_logged "$intent done")
    pal_run "$word" "${4:-}" || bad "palette ($intent): run $word"
    check "palette ($intent): the fake ran with exactly this argv" nested_wait 5 more_fake_n "$line" "$f0"
    check "palette ($intent): pal_chosen $intent" nested_wait 5 more "pal_chosen $intent" "$c0"
    check "palette ($intent): done in the log" nested_wait 3 more_pal_logged "$intent done" "$d0"
    settle
}

rm -f "$D/late_final" "$D/pal_off" "$D/pal_slow" "$D/pal_slow_ask"
echo 0 >"$KEYS"
# Diktat, Textausgabe, Textaktionen und Diagnose starten nie ein Programm der Palette
check "palette: no fake program ran before the palette tests" test ! -s "$FAKE_LOG"
: >"$FAKE_LOG"
# Fenster der Palette: Textfeld (für „kein Tippen ins Fenster“ und das Fensterdiktat), Testtext im Titel (Kontext)
TARGET_TITLE_PAL="palmarker Fenster der Palette"
nested_spawn pal env TARGET_TITLE="$TARGET_TITLE_PAL" python3 "$HERE/target.py" text "$PAL_APP"
nested_wait 10 has_win "$PAL_APP" || bad "palette: own target window"
nested_wait 5 overlay_down

# --- P1. Öffnen: schwebend, fokussiert, Eingabezeile mit Fokus ohne Markierung -------------------------------------------
focus dictate-test-target || bad "palette: focus the target window"
nested_spawn palprim "$CLIP" watch-primary
PRIMARY_PID=$NESTED_LAST_PID
nested_wait 5 grep -q 'primary #1' "$NESTED_DIR/palprim.log" || bad "palette: PRIMARY watcher"
p0=$(grep -c 'primary #' "$NESTED_DIR/palprim.log")
check "palette: Mod+Alt+Space opens the panel, focused" pal_open
check "palette: panel floats" panel_floats
check "palette: context carries the windows" grep -q ' pal_context [1-9]' "$MOCK_LOG"
check "palette: the panel itself is not the focus in the context" grep -q 'palette: opening (window known, context known)' "$UI_LOG"
check "palette: typing reaches the input line" pal_type palmarker
check "palette: no PRIMARY from focusing or typing" test "$(grep -c 'primary #' "$NESTED_DIR/palprim.log")" -eq "$p0"
kill -TERM -- "-$PRIMARY_PID" 2>/dev/null
nflush
nested_shot "$D/palette-typed.png" || true
check "palette: Esc closes" pal_close
check "palette: focus back on the window before" nested_wait 3 target_focused

# --- P2. Return startet ein Programm über das Fake (`launch_app`, sicherer Treffer) -------------------------------------
a0=$(count 'pal_ask 7') c0=$(count 'pal_chosen launch_app') f0=$(fakes dictate-fake-app)
pal_open || bad "palette (run): open"
pal_type fakeapp || bad "palette (run): type"
key return
check "palette (run): Return closes" nested_wait 5 panel_down
check "palette (run): the fake program ran with its argv" nested_wait 5 more_fakes 'dictate-fake-app \[--from-palette\]$' "$f0"
check "palette (run): pal_chosen launch_app" nested_wait 5 more 'pal_chosen launch_app' "$c0"
check "palette (run): done in the log" nested_wait 3 grep -q 'palette: launch_app done' "$UI_LOG"
check "palette (run): Return on a confident match does not ask the AI" test "$(count 'pal_ask 7')" -eq "$a0"
settle

# --- P3. Return auf dem Fenster fokussiert es (echte niri-ID aus dem Kontext) -----------------------------------------
c0=$(count 'pal_chosen focus_window')
focus dictate-test-target
pal_open || bad "palette (focus): open"
key return
check "palette (focus): Return closes" nested_wait 5 panel_down
check "palette (focus): the window has the focus" nested_wait 5 pal_window_focused
check "palette (focus): pal_chosen focus_window" nested_wait 5 more 'pal_chosen focus_window' "$c0"
settle

# --- P4. confirm: erstes Return zeigt die Bestätigung, Esc hebt sie auf, Return und Return führen aus --------------------
focus dictate-test-target
c0=$(count 'pal_chosen reboot') s0=$(fakes systemctl) k0=$(ui_has 'panel: layout confirm ')
pal_open || bad "palette (confirm): open"
pal_type reboot || bad "palette (confirm): type"
key return
check "palette (confirm): first Return shows the confirmation" nested_wait 5 more_ui 'panel: layout confirm ' "$k0"
settle
check "palette (confirm): nothing ran yet" test "$(fakes systemctl)" -eq "$s0"
check "palette (confirm): panel stays open" panel_up
nflush
nested_shot "$D/palette-confirm.png" || true
key escape
settle
check "palette (confirm): Esc cancels only the confirmation" panel_up
check "palette (confirm): still nothing ran" test "$(fakes systemctl)" -eq "$s0"
k0=$(ui_has 'panel: layout confirm ')
key return
nested_wait 5 more_ui 'panel: layout confirm ' "$k0" || bad "palette (confirm): confirmation again"
settle
key return
check "palette (confirm): second Return closes" nested_wait 5 panel_down
check "palette (confirm): the fake systemctl reboot ran" nested_wait 5 more_fakes 'systemctl \[reboot\]$' "$s0"
check "palette (confirm): pal_chosen reboot" nested_wait 5 more 'pal_chosen reboot' "$c0"
settle

# --- P5. Return vor der Antwort wartet auf sie (langsamer Daemon) ------------------------------------------------------
touch "$D/pal_slow"
focus dictate-test-target
f0=$(fakes dictate-fake-app)
pal_open || bad "palette (slow): open"
r0=$(count 'pal_resolve 7')
key text:fakeapp
key return
check "palette (slow): Return before the answer runs after it" nested_wait 10 more_fakes 'dictate-fake-app ' "$f0"
check "palette (slow): closed after running" nested_wait 3 panel_down
check "palette (slow): the whole input reached the daemon" more 'pal_resolve 7' "$r0"
rm -f "$D/pal_slow"
settle

# --- P6. Ausgeschaltet: neutrale Meldung statt Liste -------------------------------------------------------------------
touch "$D/pal_off"
focus dictate-test-target
e0=$(pal_renders Error)
pal_open || bad "palette (off): open"
check "palette (off): the daemon's notice is shown" nested_wait 3 more_pal_renders Error "$e0"
nflush
nested_shot "$D/palette-off.png" || true
pal_close || bad "palette (off): close"
rm -f "$D/pal_off"

# --- P7. Ctrl+Return fragt die KI, Esc legt auf; danach läuft der Vorschlag ---------------------------------------------
touch "$D/pal_slow_ask"
focus dictate-test-target
a0=$(count 'pal_ask 9') h0=$(count 'pal_ask hung up') w0=$(pal_renders Asking)
pal_open || bad "palette (ask): open"
pal_type palmarker || bad "palette (ask): type"
key ctrl+return
check "palette (ask): Ctrl+Return asks the AI" nested_wait 5 more 'pal_ask 9' "$a0"
check "palette (ask): waiting view" nested_wait 3 more_pal_renders Asking "$w0"
sleep 0.5
key escape
check "palette (ask): Esc hangs up" nested_wait 5 more 'pal_ask hung up' "$h0"
check "palette (ask): the panel stays open after hanging up" panel_up
rm -f "$D/pal_slow_ask"
settle
o0=$(ui_has 'palette pal_ask: ok after') f0=$(fakes dictate-fake-app) c0=$(count 'pal_chosen launch_app')
key ctrl+return
check "palette (ask): the AI answers" nested_wait 5 more_ui 'palette pal_ask: ok after' "$o0"
settle
nflush
nested_shot "$D/palette-ai.png" || true
key return
check "palette (ask): Return runs the suggestion" nested_wait 5 more_fakes 'dictate-fake-app \[--from-palette\]$' "$f0"
check "palette (ask): pal_chosen for the suggestion" nested_wait 5 more 'pal_chosen launch_app' "$c0"
nested_wait 3 panel_down
settle

# --- P8. Stimme ins Panel: Text in der Zeile, kein Ausführen, kein Tippen ins Fenster, Overlay aus ----------------------
focus "$PAL_APP" || bad "palette (voice): focus the window"
text0=$(text_of pal) f0=$(wc -l <"$FAKE_LOG") ch0=$(grep -c ' pal_chosen ' "$MOCK_LOG")
t0=$(count 'start_target palette') v0=$(pal_renders Voice) q0=$(count "pal_resolve $FINAL_LEN") w0=$(ui_has 'recording for the window')
nested_wait 5 overlay_down
pal_open || bad "palette (voice): open"
echo 1 >"$KEYS"
press alt+e
check "palette (voice): Mod+E records for the palette" nested_wait 3 more 'start_target palette' "$t0"
check "palette (voice): owner in the log" nested_wait 3 grep -q 'recording for the command palette$' "$UI_LOG"
check "palette (voice): recording view" nested_wait 3 more_pal_renders Voice "$v0"
sleep 0.6 # Pegel und Mitschrift laufen
check "palette (voice): overlay stays hidden" overlay_down
nflush
nested_shot "$D/palette-voice.png" || true
echo 0 >"$KEYS"
check "palette (voice): the final text fills the input line" nested_wait 5 more "pal_resolve $FINAL_LEN" "$q0"
check "palette (voice): final for the palette in the log" grep -q "final: $FINAL_LEN chars for the command palette\$" "$UI_LOG"
sleep 0.8
check "palette (voice): panel stays open" panel_up
check "palette (voice): nothing ran" test "$(wc -l <"$FAKE_LOG")" -eq "$f0" -a "$(grep -c ' pal_chosen ' "$MOCK_LOG")" -eq "$ch0"
check "palette (voice): nothing typed into the window" test "$(text_of pal)" = "$text0"
check "palette (voice): no recording for the window" test "$(ui_has 'recording for the window')" -eq "$w0"
check "palette (voice): overlay still hidden" overlay_down
pal_close || bad "palette (voice): close"

# --- P9. Esc bricht die Aufnahme ab, ein spätes `final` wird verworfen ---------------------------------------------------
touch "$D/late_final"
t0=$(count 'start_target palette') v0=$(pal_renders Voice) c0=$(count cancel) s0=$(count stop) l0=$(count "pal_resolve $LATE_LEN")
d0=$(ui_has 'of a cancelled palette recording, discarded')
pal_open || bad "palette (voice cancel): open"
echo 1 >"$KEYS"
press alt+e
nested_wait 3 more 'start_target palette' "$t0" || bad "palette (voice cancel): start"
nested_wait 3 more_pal_renders Voice "$v0" || bad "palette (voice cancel): recording view"
sleep 0.4
key escape
check "palette (voice cancel): Esc cancels the recording" nested_wait 3 more cancel "$c0"
check "palette (voice cancel): the late final is discarded" nested_wait 3 more_ui 'of a cancelled palette recording, discarded' "$d0"
echo 0 >"$KEYS"
sleep 0.5
check "palette (voice cancel): the late text never reaches the input line" test "$(count "pal_resolve $LATE_LEN")" -eq "$l0"
check "palette (voice cancel): releasing E sends no stop" test "$(count stop)" -eq "$s0"
check "palette (voice cancel): the panel stays open" panel_up
check "palette (voice cancel): nothing typed into the window" test "$(text_of pal)" = "$text0"
pal_close || bad "palette (voice cancel): close"
rm -f "$D/late_final"

# --- P10. Danach ein Fensterdiktat ins Zielfenster (der Besitz kehrt zum Fenster zurück) -------------------------------
focus "$PAL_APP" || bad "palette (window dictation): focus"
w0=$(ui_has 'recording for the window') n0=$(tlen pal)
echo 1 >"$KEYS"
press alt+e
check "palette (window dictation): recording for the window" nested_wait 3 more_ui 'recording for the window$' "$w0"
check "palette (window dictation): overlay shown" nested_wait 3 overlay_up
echo 0 >"$KEYS"
check "palette (window dictation): text in the window" nested_wait 8 pal_len_is $((n0 + FINAL_LEN))
check "palette (window dictation): the final text" pal_ends_with_final
nested_wait 5 overlay_down

# --- P11. Ein Panel zur Zeit nach benutzter Palette: die verborgene Eingabezeile fängt keine Tasten mehr --------------------
focus dictate-test-target
pal_open || bad "palette (one panel): open"
pal_type palmarker || bad "palette (one panel): type"
pal_close || bad "palette (one panel): close"
nested_spawn palsel python3 "$HERE/target.py" text dictate-test-palsel "$SEL"
nested_wait 10 has_win dictate-test-palsel || bad "palette (one panel): selection window"
nested_wait 5 grep -q '^selected' "$NESTED_DIR/palsel.log"
check "palette (one panel): Mod+I opens the text actions" open_panel dictate-test-palsel
settle
r0=$(runs)
key e
check "palette (one panel): a letter reaches the text actions" run_done "$r0"
key escape
nested_wait 5 panel_down
settle
focus dictate-test-target
check "palette (one panel): Mod+Shift+I opens the diagnosis" diag_open
check "palette (one panel): a digit reaches the diagnosis" diag_collect
settle
s0=$(count 'diag_symptom 3')
key tab
sleep 0.3
key text:abc
sleep 0.3
key return
check "palette (one panel): Tab and letters reach the field of the diagnosis" nested_wait 5 more 'diag_symptom 3' "$s0"
settle
key escape
settle
key escape
nested_wait 5 panel_down
settle

# --- P13. Ein Panel zur Zeit über die Kürzel: jedes Kürzel schließt das offene andere Panel nur ------------------------
focus dictate-test-palsel
pal_open || bad "palette (shortcuts): open"
key alt+space
check "palette (shortcuts): Mod+Alt+Space closes the open palette" nested_wait 5 panel_down
settle
pal_open || bad "palette (shortcuts): open for Mod+I"
g0=$(acts act_grab) k0=$(ui_has 'text actions: the command palette is open, closing it')
key alt+i
check "palette (shortcuts): Mod+I closes the palette" nested_wait 5 panel_down
check "palette (shortcuts): Mod+I names it in the log" more_ui 'text actions: the command palette is open, closing it' "$k0"
sleep 0.5
check "palette (shortcuts): Mod+I opened no text actions" test "$(acts act_grab)" -eq "$g0" -a -z "$(panel_json)"
settle
pal_open || bad "palette (shortcuts): open for Mod+Shift+I"
p0=$(count diag_plan) k0=$(ui_has 'diagnose: the command palette is open, closing it')
diag_key
check "palette (shortcuts): Mod+Shift+I closes the palette" nested_wait 5 panel_down
check "palette (shortcuts): Mod+Shift+I names it in the log" more_ui 'diagnose: the command palette is open, closing it' "$k0"
sleep 0.5
check "palette (shortcuts): Mod+Shift+I opened no diagnosis" test "$(count diag_plan)" -eq "$p0" -a -z "$(panel_json)"
settle
open_panel dictate-test-palsel || bad "palette (shortcuts): text actions open"
r0=$(count 'pal_resolve 0') k0=$(ui_has 'palette: the text actions panel is open, closing it')
key alt+space
check "palette (shortcuts): Mod+Alt+Space closes the text actions" nested_wait 5 panel_down
check "palette (shortcuts): Mod+Alt+Space names the text actions in the log" more_ui 'palette: the text actions panel is open, closing it' "$k0"
sleep 0.5
check "palette (shortcuts): no palette after closing the text actions" test "$(count 'pal_resolve 0')" -eq "$r0" -a -z "$(panel_json)"
settle
focus dictate-test-palsel
diag_open || bad "palette (shortcuts): diagnosis open"
r0=$(count 'pal_resolve 0') k0=$(ui_has 'palette: the diagnosis is open, closing it')
key alt+space
check "palette (shortcuts): Mod+Alt+Space closes the diagnosis" nested_wait 5 panel_down
check "palette (shortcuts): Mod+Alt+Space names the diagnosis in the log" more_ui 'palette: the diagnosis is open, closing it' "$k0"
sleep 0.5
check "palette (shortcuts): no palette after closing the diagnosis" test "$(count 'pal_resolve 0')" -eq "$r0" -a -z "$(panel_json)"
settle

# --- P14. ask: die Frage geht an das Panel der Textaktionen, die Antwort startet sofort, pal_chosen danach -----------
PAL_QUESTION='palmarker Frage: was ist ein Monad?' # PAL_QUESTION des Mocks
question_exact() { [[ "$(cat "$D/grab.txt")" == "$PAL_QUESTION" ]]; }
focus dictate-test-target
c0=$(count 'pal_chosen ask') g0=$(acts grab_question) a0=$(acts 'act_run ask') r0=$(runs) h0=$(ui_has 'palette: ask handed over to the text actions panel (question)')
pal_open || bad "palette (ask): open"
pal_type question || bad "palette (ask): type"
key return
check "palette (ask): handed over to the text actions panel" nested_wait 5 more_ui 'palette: ask handed over to the text actions panel (question)' "$h0"
check "palette (ask): act_grab with the source question" nested_wait 5 more_acts grab_question "$g0"
check "palette (ask): the daemon got the question exactly" question_exact
check "palette (ask): the answer starts without a choice" nested_wait 5 more_acts 'act_run ask' "$a0"
check "palette (ask): the answer arrives" run_done "$r0"
check "palette (ask): pal_chosen ask after the answer" nested_wait 5 more 'pal_chosen ask' "$c0"
check "palette (ask): the panel is up and focused" panel_focused
# das Wachsen über niri braucht etwas länger als das letzte Bild von GTK
sleep 1
nflush
sleep 0.3
nested_shot "$D/palette-ask.png" || true
key escape
check "palette (ask): Esc closes" nested_wait 5 panel_down
settle

# --- P15a. Gegenprobe zu P15 (L35): eine gehaltene Taste des Injektors wiederholt im verschachtelten niri -----------------
# Sonst bewiese „ein gehaltenes Return sendet nichts“ nichts: ohne Wiederholung käme ohnehin nur ein Druck an
rep_seen() { since "$1" | grep -qE ' pal_resolve ([2-9]|[1-9][0-9]+)$'; }
focus dictate-test-target
pal_open || bad "palette (repeat): open"
t0=$(now_ns)
key down:a
sleep 1.5
key up:a
check "palette (repeat): a held key repeats in the nested niri (pal_resolve with 2 or more chars)" nested_wait 5 rep_seen "$t0"
settle
echo "info palette (repeat): longest input after holding a for 1.5 s: $(since "$t0" | sed -n 's/.* pal_resolve \([0-9]*\)$/\1/p' | sort -n | tail -n 1)"
pal_close || bad "palette (repeat): close"

# --- P15. explain_clipboard: Vorschau, ein gehaltenes Return sendet nichts, erst ein neues Return ------------------------
# CLIPBOARD gehört einem eigenen Anbieter (nested-clip offer); gelesen wird sie nur auf diese Absicht
PAL_CLIP='palmarker Inhalt der Zwischenablage: äöü ß'
clip_exact() { [[ "$(cat "$D/grab.txt")" == "$PAL_CLIP" ]]; }
mkdir -p "$D/palclip" "$D/palclip-secret"
printf 'text/plain;charset=utf-8\ntext/plain\n' >"$D/palclip/types.txt"
printf '%s' "$PAL_CLIP" >"$D/palclip/0.bin"
printf '%s' "$PAL_CLIP" >"$D/palclip/1.bin"
mkfifo "$D/palclip.in"
exec {PCFD}<>"$D/palclip.in"
nested_spawn palclip "$CLIP" offer "$D/palclip" <"$D/palclip.in"
nested_wait 5 grep -q 'offering 2 types' "$NESTED_DIR/palclip.log" || bad "palette (clipboard): clipboard offered"
check "palette (clipboard): nothing read the clipboard before the intent" bash -c "! grep -qE 'send #[0-9]+ ' '$NESTED_DIR/palclip.log'"
focus dictate-test-target
c0=$(count 'pal_chosen explain_clipboard') g0=$(acts grab_clipboard) e0=$(acts 'act_run explain') r0=$(runs)
pal_open || bad "palette (clipboard): open"
pal_type clip || bad "palette (clipboard): type"
# Öffnen, Tippen und Auflösen lesen CLIPBOARD ebenso wenig (erst die Absicht nach Return)
check "palette (clipboard): nothing read the clipboard while typing the intent" bash -c "! grep -qE 'send #[0-9]+ ' '$NESTED_DIR/palclip.log'"
key down:return
check "palette (clipboard): act_grab with the source clipboard" nested_wait 5 more_acts grab_clipboard "$g0"
check "palette (clipboard): the daemon got the clipboard exactly" clip_exact
# länger als Totzeit und Verzögerung der Tastenwiederholung: ein gehaltenes Return bestätigt die Vorschau nie
sleep 1.5
check "palette (clipboard): a held Return sends nothing" test "$(acts 'act_run explain')" -eq "$e0"
key up:return
settle
check "palette (clipboard): still nothing sent after releasing" test "$(acts 'act_run explain')" -eq "$e0"
check "palette (clipboard): the preview is up" panel_focused
check "palette (clipboard): no pal_chosen before the answer" test "$(count 'pal_chosen explain_clipboard')" -eq "$c0"
nflush
sleep 0.3
nested_shot "$D/palette-clipboard.png" || true
key return
check "palette (clipboard): a new Return sends" nested_wait 5 more_acts 'act_run explain' "$e0"
check "palette (clipboard): the answer arrives" run_done "$r0"
check "palette (clipboard): pal_chosen explain_clipboard after the answer" nested_wait 5 more 'pal_chosen explain_clipboard' "$c0"
key escape
nested_wait 5 panel_down
settle
# Esc in der Vorschau sendet nichts
e0=$(acts 'act_run explain') g0=$(acts grab_clipboard)
pal_open || bad "palette (clipboard esc): open"
pal_type clip || bad "palette (clipboard esc): type"
key return
nested_wait 5 more_acts grab_clipboard "$g0" || bad "palette (clipboard esc): grab"
settle
key escape
check "palette (clipboard esc): Esc closes the preview" nested_wait 5 panel_down
sleep 0.3
check "palette (clipboard esc): nothing sent" test "$(acts 'act_run explain')" -eq "$e0"
settle

# --- P16. Passwort in der Zwischenablage: nur der Hinweis wird gelesen, nichts geht an den Daemon --------------------------
printf 'text/plain;charset=utf-8\ntext/plain\nx-kde-passwordManagerHint\n' >"$D/palclip-secret/types.txt"
printf 'Passwort-der-Palette' >"$D/palclip-secret/0.bin"
printf 'Passwort-der-Palette' >"$D/palclip-secret/1.bin"
printf 'secret' >"$D/palclip-secret/2.bin"
mkfifo "$D/palsecret.in"
exec {PSFD}<>"$D/palsecret.in"
nested_spawn palsecret "$CLIP" offer "$D/palclip-secret" <"$D/palsecret.in"
nested_wait 5 grep -q 'offering 3 types' "$NESTED_DIR/palsecret.log" || bad "palette (password): clipboard offered"
g0=$(acts grab_clipboard) f0=$(ui_has 'text actions: grab failed: The clipboard holds a password')
pal_open || bad "palette (password): open"
pal_type clip || bad "palette (password): type"
key return
check "palette (password): refused in the panel" nested_wait 5 more_ui 'text actions: grab failed: The clipboard holds a password' "$f0"
check "palette (password): nothing went to the daemon" test "$(acts grab_clipboard)" -eq "$g0"
check "palette (password): only the hint was read, never the password" bash -c "! grep -E 'send #[0-9]+ ' '$NESTED_DIR/palsecret.log' | grep -qv 'x-kde-passwordManagerHint'"
nflush
sleep 0.3
nested_shot "$D/palette-password.png" || true
key escape
nested_wait 5 panel_down
settle
exec {PSFD}>&- {PCFD}>&-

# --- P16b. CLIPBOARD aus eigener Quelle (Copy der Textaktionen): der Text kommt aus dem Speicher, ohne auf die Frist zu warten --
focus dictate-test-palsel || bad "palette (own clipboard): focus the selection window"
open_panel dictate-test-palsel || bad "palette (own clipboard): text actions open"
settle
r0=$(runs) l0=$(copies)
key e
result_shown "$r0" "$l0" || bad "palette (own clipboard): explain"
settle
key return
nested_wait 5 panel_down || bad "palette (own clipboard): copy closes the panel"
settle
check "palette (own clipboard): CLIPBOARD holds the copied answer" clip_is_result
g0=$(acts grab_clipboard) f0=$(ui_has 'text actions: grab failed')
pal_open || bad "palette (own clipboard): open"
pal_type clip || bad "palette (own clipboard): type"
t0=$(now_ns)
key return
check "palette (own clipboard): act_grab with the source clipboard" nested_wait 5 more_acts grab_clipboard "$g0"
dt=$((($(since "$t0" | grep -m 1 ' grab_clipboard ' | cut -d' ' -f1) - t0) / 1000000))
# die Frist des Lesens ist 2 s (actions::selection::TIMEOUT); aus dem Speicher deutlich darunter
if ((dt < 1000)); then ok "palette (own clipboard): grabbed fast ($dt ms)"; else bad "palette (own clipboard): grabbed fast ($dt ms, want < 1000)"; fi
check "palette (own clipboard): the daemon got the copied answer exactly" cmp -s "$D/grab.txt" "$D/result.txt"
check "palette (own clipboard): no grab error" test "$(ui_has 'text actions: grab failed')" -eq "$f0"
settle
key escape
nested_wait 5 panel_down
settle

# --- P17. text_action über die KI: Vorschlag, Return, Mod+I-Ablauf auf PRIMARY mit Vorschau; erst ein neues Return sendet
#     die vorgewählte Aktion (Entscheidung 18: PRIMARY geht nie ungesehen an die KI) --------------------------------------
focus dictate-test-palsel || bad "palette (text action): focus the selection window"
c0=$(count 'pal_chosen text_action') g0=$(acts 'act_grab [0-9]* dictate-test-palsel') q0=$(count 'pal_ask 6') a0=$(acts 'act_run rephrase') l0=$(ui_has 'panel: layout replace ')
p0=$(ui_has 'panel: layout action:rephrase ')
pal_open || bad "palette (text action): open"
pal_type polish || bad "palette (text action): type"
key return
check "palette (text action): Return asks the AI" nested_wait 5 more 'pal_ask 6' "$q0"
settle
key return
check "palette (text action): the selection is grabbed with the app_id" nested_wait 5 more_acts 'act_grab [0-9]* dictate-test-palsel' "$g0"
check "palette (text action): the daemon got PRIMARY exactly" grab_exact
check "palette (text action): the preview with the actions is up" nested_wait 5 more_ui 'panel: layout action:rephrase ' "$p0"
sleep 0.8
check "palette (text action): nothing is sent before the confirmation" test "$(acts 'act_run rephrase')" -eq "$a0"
check "palette (text action): no pal_chosen before the confirmation" test "$(count 'pal_chosen text_action')" -eq "$c0"
settle
key return
check "palette (text action): Return sends the preselected action" nested_wait 5 more_acts 'act_run rephrase' "$a0"
check "palette (text action): pal_chosen text_action after the result" nested_wait 5 more 'pal_chosen text_action' "$c0"
check "palette (text action): Replace is offered (known window, no terminal)" nested_wait 3 more_ui 'panel: layout replace ' "$l0"
key escape
nested_wait 5 panel_down
settle

# --- P18. diagnose mit Vorbelegung: Profil unit und Suchbegriff; pal_chosen erst mit den Profilen; aus = keine Wahl --------
focus dictate-test-target
c0=$(count 'pal_chosen diagnose') u0=$(count 'diag_plan_profile unit') h0=$(ui_has 'palette: diagnose handed over to the diagnosis panel')
pal_open || bad "palette (diagnose): open"
pal_type diagunit || bad "palette (diagnose): type"
key return
check "palette (diagnose): handed over to the diagnosis panel" nested_wait 5 more_ui 'palette: diagnose handed over to the diagnosis panel' "$h0"
check "palette (diagnose): pal_chosen diagnose" nested_wait 5 more 'pal_chosen diagnose' "$c0"
check "palette (diagnose): the preset picks the profile unit" nested_wait 5 more 'diag_plan_profile unit' "$u0"
check "palette (diagnose): the panel is up and focused" nested_wait 3 panel_focused
nflush
sleep 0.3
nested_shot "$D/palette-diagnose.png" || true
key escape
nested_wait 5 panel_down
settle
touch "$D/diag_off"
c0=$(count 'pal_chosen diagnose') h0=$(ui_has 'palette: diagnose handed over to the diagnosis panel')
pal_open || bad "palette (diagnose off): open"
pal_type diagunit || bad "palette (diagnose off): type"
key return
nested_wait 5 more_ui 'palette: diagnose handed over to the diagnosis panel' "$h0" || bad "palette (diagnose off): handed over"
sleep 0.8
check "palette (diagnose off): no pal_chosen" test "$(count 'pal_chosen diagnose')" -eq "$c0"
check "palette (diagnose off): the panel shows it" panel_up
rm -f "$D/diag_off"
key escape
nested_wait 5 panel_down
settle

# --- P19. Programme, Ordner und Adressen: jede Absicht startet ihr Fake mit genau diesem argv (niri `Spawn`) -----------
focus dictate-test-target
PAL_PROJECT_DIR="$D/palmarker-projekt" # PAL_PROJECT des Mocks
mkdir -p "$PAL_PROJECT_DIR"
# ohne palette.terminal und palette.project_command: das erste Terminal aus der Tabelle mit seinem Ordnerschalter, darin
# die Shell (ohne SHELL im Testmodus sh aus dem Fake-Ordner), gestartet über env -C im Projektordner
pal_expect qqproj open_project "alacritty [--working-directory] [$PAL_PROJECT_DIR] [-e] [$FAKES/sh]"
check "palette (open_project): the terminal starts in the project folder" nested_wait 3 grep -qxF "alacritty-cwd [$PAL_PROJECT_DIR]" "$FAKE_LOG"
# mit beiden aus der Config (der Daemon schickt sie mit jeder Antwort): genau dieses argv, der Befehl als absoluter Pfad
cp "$D/config.json" "$D/config.json.pal"
python3 -I -c 'import json, sys
p = sys.argv[1]
c = json.load(open(p))
c["palette"] = {"terminal": ["kitty", "--single-instance"], "project_command": ["claude", "--continue"]}
json.dump(c, open(p, "w"))' "$D/config.json"
pal_expect qqproj open_project "kitty [--single-instance] [$FAKES/claude] [--continue]"
mv "$D/config.json.pal" "$D/config.json"
pal_expect qqfolder open_folder "xdg-open [$PAL_PROJECT_DIR]"
pal_expect qqurl open_url "xdg-open [https://example.org/palmarker-seite]"
# Suchbegriff mit Leerzeichen, &, # und Umlauten: nach RFC 3986 kodiert, ein einziges argv-Element
PAL_QUERY='palmarker Suche & mehr #1 äöü' # PAL_QUERY des Mocks
PAL_QUERY_ENC=$(python3 -c 'import sys, urllib.parse; print(urllib.parse.quote(sys.argv[1], safe="-._~"))' "$PAL_QUERY")
pal_expect qqsearch web_search "xdg-open [https://example.org/?q=$PAL_QUERY_ENC]"
check "palette (web_search): the query is encoded (no space, & or #)" test "$PAL_QUERY_ENC" = 'palmarker%20Suche%20%26%20mehr%20%231%20%C3%A4%C3%B6%C3%BC'
pal_expect qqemoji show_widget "rofi [-modi] [emoji] [-show] [emoji]"
pal_expect qqsettings dictate_settings "dictate-settings [--page] [palette]"

# --- P20. System: Kurzläufer mit festem Programmnamen (Sperre, Lautstärke, Medien, Verlauf der Zwischenablage) ---------
focus dictate-test-target
# Sperre über logind (PLAN-lizenz.md O6): ohne Argument die Sitzung `auto`
pal_expect qqlock lock "loginctl [lock-session]"
pal_expect qqcliphist show_widget "noctalia [msg] [panel-toggle] [clipboard]"
# eigene Befehle für Zwischenablage und Emoji (palette.clipboard_command, palette.emoji_command; der Daemon schickt sie
# mit jeder Antwort): genau dieses argv, über niri gestartet statt Noctalia bzw. rofi
cp "$D/config.json" "$D/config.json.pal"
python3 -I -c 'import json, sys
p = sys.argv[1]
c = json.load(open(p))
c["palette"] = {"clipboard_command": ["copyq", "toggle"], "emoji_command": ["bemoji", "-t"]}
json.dump(c, open(p, "w"))' "$D/config.json"
pal_expect qqcliphist show_widget "copyq [toggle]"
pal_expect qqemoji show_widget "bemoji [-t]"
mv "$D/config.json.pal" "$D/config.json"
pal_expect qqmedia media "playerctl [play-pause]"
pal_expect qqvolset volume "pactl [set-sink-volume] [@DEFAULT_SINK@] [40%]"
pal_expect qqmute volume "pactl [set-sink-mute] [@DEFAULT_SINK@] [1]"
# +10 bei 95 %: erst lesen, dann nur bis 100 % (relativ)
printf 'Volume: front-left: 62259 /  95%% / -1.34 dB,   front-right: 62259 /  95%% / -1.34 dB\n        balance 0.00\n' >"$D/pactl-volume"
g0=$(fake_n "pactl [get-sink-volume] [@DEFAULT_SINK@]")
pal_expect qqvolup volume "pactl [set-sink-volume] [@DEFAULT_SINK@] [+5%]"
check "palette (volume): read before raising" test "$(fake_n "pactl [get-sink-volume] [@DEFAULT_SINK@]")" -gt "$g0"
# bei 100 % nichts setzen, nur der Hinweis im Overlay
printf 'Volume: front-left: 65536 / 100%% / 0.00 dB,   front-right: 65536 / 100%% / 0.00 dB\n' >"$D/pactl-volume"
nested_wait 8 overlay_down
s0=$(grep -c '^pactl \[set-sink-volume\]' "$FAKE_LOG") g0=$(fake_n "pactl [get-sink-volume] [@DEFAULT_SINK@]") d0=$(pal_logged "volume done")
pal_run qqvolup || bad "palette (volume 100): run"
check "palette (volume 100): read the volume" nested_wait 5 more_fake_n "pactl [get-sink-volume] [@DEFAULT_SINK@]" "$g0"
check "palette (volume 100): done in the log" nested_wait 3 more_pal_logged "volume done" "$d0"
check "palette (volume 100): the notice is in the overlay" nested_wait 3 overlay_up
sleep 0.3
check "palette (volume 100): nothing set above 100 %" test "$(grep -c '^pactl \[set-sink-volume\]' "$FAKE_LOG")" -eq "$s0"
nested_wait 8 overlay_down
settle

# --- P21. confirm: poweroff erst nach dem zweiten Return; suspend scheitert (auf blub gesperrt) und meldet es ---------------
focus dictate-test-target
pal_no_poweroff_yet() { test "$(fake_n 'systemctl [poweroff]')" -eq "$p0"; }
p0=$(fake_n 'systemctl [poweroff]')
PAL_BEFORE_CONFIRM=pal_no_poweroff_yet pal_expect qqpoweroff poweroff "systemctl [poweroff]" confirm
s0=$(fake_n 'systemctl [suspend]') c0=$(pal_chosen_n suspend) e0=$(pal_logged "suspend failed")
nested_wait 8 overlay_down
pal_run qqsuspend confirm || bad "palette (suspend): run"
check "palette (suspend): systemctl suspend was called" nested_wait 5 more_fake_n 'systemctl [suspend]' "$s0"
check "palette (suspend): failed in the log" nested_wait 3 more_pal_logged "suspend failed" "$e0"
check "palette (suspend): the error is in the overlay" nested_wait 3 overlay_up
nflush
nested_shot "$D/palette-suspend.png" || true
check "palette (suspend): no pal_chosen after a failure" test "$(pal_chosen_n suspend)" -eq "$c0"
nested_wait 8 overlay_down
settle

# --- P22. Arbeitsflächen und Fokus: focus_window über Arbeitsflächen, view_workspace, previous_workspace (niri-IDs) -----------
WS_APP=dictate-test-ws # PAL_WS_APP des Mocks
wjs() { # APP AUSDRUCK: Feld des Fensters mit dieser app_id (w = Fenster)
    nmsg -j windows | python3 -c 'import json,sys
w=next((w for w in json.load(sys.stdin) if w.get("app_id")==sys.argv[1]), None)
print(eval(sys.argv[2]) if w else "")' "$1" "$2"
}
focused_ws_idx() { nmsg -j workspaces | python3 -c 'import json,sys; print(next((w["idx"] for w in json.load(sys.stdin) if w["is_focused"]), ""))'; }
ws_idx_of() { # APP: idx der Arbeitsfläche des Fensters
    local ws
    ws=$(wjs "$1" 'w["workspace_id"]')
    nmsg -j workspaces | python3 -c 'import json,sys; print(next((w["idx"] for w in json.load(sys.stdin) if str(w["id"])==sys.argv[1]), ""))' "$ws"
}
ws_idx_is() { [[ $(focused_ws_idx) == "$1" ]]; }
win_ws_is() { [[ $(ws_idx_of "$1") == "$2" ]]; }
focused_app_is() { [[ $(focused_app) == "$1" ]]; }
nested_spawn palws python3 "$HERE/target.py" text "$WS_APP"
WS_PID=$NESTED_LAST_PID
nested_wait 10 has_win "$WS_APP" || bad "palette (workspaces): window for the workspaces"
WS_ID=$(win_id "$WS_APP")
focus dictate-test-target || bad "palette (workspaces): focus the target"
nmsg action move-window-to-workspace --window-id "$WS_ID" --focus false 2
check "palette (workspaces): set-up: the window sits on workspace 2" nested_wait 3 win_ws_is "$WS_APP" 2
check "palette (workspaces): set-up: the target on workspace 1 keeps the focus" nested_wait 3 focused_app_is dictate-test-target
check "palette (workspaces): set-up: workspace 1 is focused" ws_idx_is 1

# focus_window auf ein Fenster einer anderen Arbeitsfläche: nimmt niri die Arbeitsfläche mit?
c0=$(pal_chosen_n focus_window)
pal_run qqfocus || bad "palette (focus other workspace): run"
check "palette (focus other workspace): the window has the focus" nested_wait 5 focused_app_is "$WS_APP"
check "palette (focus other workspace): niri switched to its workspace (2)" nested_wait 3 ws_idx_is 2
check "palette (focus other workspace): pal_chosen focus_window" nested_wait 5 more "pal_chosen focus_window" "$c0"
settle

# view_workspace: Arbeitsfläche 2 vom Zielfenster aus
focus dictate-test-target || bad "palette (view_workspace): focus the target"
nested_wait 3 ws_idx_is 1 || bad "palette (view_workspace): back on workspace 1"
c0=$(pal_chosen_n view_workspace)
pal_run qqview || bad "palette (view_workspace): run"
check "palette (view_workspace): workspace 2 is focused" nested_wait 5 ws_idx_is 2
check "palette (view_workspace): its window has the focus" nested_wait 3 focused_app_is "$WS_APP"
check "palette (view_workspace): pal_chosen view_workspace" nested_wait 5 more "pal_chosen view_workspace" "$c0"
settle

# previous_workspace: zurück auf 1 (Öffnen und Schließen der Palette zählen nicht als Wechsel)
c0=$(pal_chosen_n previous_workspace)
pal_run qqprev || bad "palette (previous_workspace): run"
check "palette (previous_workspace): back on workspace 1" nested_wait 5 ws_idx_is 1
check "palette (previous_workspace): pal_chosen previous_workspace" nested_wait 5 more "pal_chosen previous_workspace" "$c0"
settle

# --- P23. column_display (L15): das Ziel liegt auf einer anderen Arbeitsfläche und hat vor dem Öffnen nicht den Fokus ---------
# Zwei Fenster in einer Spalte: normal teilen sie die Höhe, tabbed hat jedes die volle (sichtbare Wirkung über IPC)
WS2_APP=dictate-test-ws2
nested_spawn palws2 python3 "$HERE/target.py" text "$WS2_APP"
WS2_PID=$NESTED_LAST_PID
nested_wait 10 has_win "$WS2_APP" || bad "palette (column): second window"
WS2_ID=$(win_id "$WS2_APP")
nmsg action move-window-to-workspace --window-id "$WS2_ID" --focus false 2
nested_wait 3 win_ws_is "$WS2_APP" 2 || bad "palette (column): second window on workspace 2"
focus "$WS2_APP" && nmsg action consume-or-expel-window-left
col_of() { wjs "$1" 'w["layout"]["pos_in_scrolling_layout"][0]'; }
same_column() { [[ -n $(col_of "$WS_APP") && $(col_of "$WS_APP") == $(col_of "$WS2_APP") ]]; }
check "palette (column): set-up: both windows in one column" nested_wait 3 same_column
tile_h() { wjs "$WS_APP" 'round(w["layout"]["tile_size"][1])'; }
OUT_H=$(nmsg -j outputs | python3 -c 'import json,sys; print(round(next(iter(json.load(sys.stdin).values()))["logical"]["height"]))')
col_shared() { local h; h=$(tile_h); ((h > 0 && h < OUT_H * 3 / 4)); }
col_full() { local h; h=$(tile_h); ((h >= OUT_H * 3 / 4)); }
# niri legt die Spalte nach dem Zusammenführen neu: erst messen, wenn sie geteilt ist (sonst die alte volle Höhe)
check "palette (column): set-up: the two windows share the column height" nested_wait 3 col_shared
H_NORMAL=$(tile_h)
taken=0 rounds=3
for i in $(seq "$rounds"); do
    for mode in tabbed normal; do
        focus dictate-test-target || bad "palette (column $mode $i): focus the target"
        nested_wait 3 ws_idx_is 1
        d0=$(pal_logged "column_display done") f0=$(pal_logged "column_display failed") c0=$(pal_chosen_n column_display)
        pal_run "qq$mode" || bad "palette (column $mode $i): run"
        if nested_wait 5 more_pal_logged "column_display done" "$d0"; then
            taken=$((taken + 1))
            check "palette (column $mode $i): pal_chosen column_display" nested_wait 5 more "pal_chosen column_display" "$c0"
        else
            nested_wait 3 more_pal_logged "column_display failed" "$f0" && echo "info palette (column $mode $i): failed (the window did not take the focus)"
        fi
        # auf die Wirkung warten (Frist), dann messen; die Prüfung unten bewertet die Werte
        [[ $mode == tabbed ]] && { nested_wait 3 col_full; H_TABBED=$(tile_h); }
        [[ $mode == normal ]] && { nested_wait 3 col_shared; H_BACK=$(tile_h); }
        settle
    done
done
echo "info palette (column, L15): FocusedWindow right after Handled in $taken of $((2 * rounds)) runs; tile height normal $H_NORMAL, tabbed $H_TABBED, back $H_BACK"
check "palette (column, L15): niri reports the new focus right after Handled ($taken/$((2 * rounds)))" test "$taken" -eq $((2 * rounds))
check "palette (column): tabbed gives the window the full column height ($H_NORMAL → $H_TABBED)" test "${H_TABBED:-0}" -gt $((${H_NORMAL:-0} * 3 / 2))
check "palette (column): normal shares the height again ($H_BACK)" test "${H_BACK:-0}" -eq "${H_NORMAL:-1}"
kill -TERM -- "-$WS2_PID" 2>/dev/null
nested_wait 5 bash -c "! nmsg -j windows | grep -q '\"$WS2_APP\"'" || bad "palette (column): second window closed"

# --- P24. move_window_to_workspace: das Fenster wandert auf 1 zum Zielfenster, der Fokus bleibt ---------------------------
focus dictate-test-target || bad "palette (move): focus the target"
c0=$(pal_chosen_n move_window_to_workspace)
pal_run qqmove || bad "palette (move): run"
check "palette (move): the window is on workspace 1" nested_wait 5 win_ws_is "$WS_APP" 1
check "palette (move): the focus stays on the target" focused_app_is dictate-test-target
check "palette (move): pal_chosen move_window_to_workspace" nested_wait 5 more "pal_chosen move_window_to_workspace" "$c0"
settle

# --- P25. toggle_window (floating, fullscreen, maximized) auf ein nicht fokussiertes Fenster, je hin und zurück ---------------
ws_w() { wjs "$WS_APP" 'round(w["layout"]["window_size"][0])'; }
ws_tile() { wjs "$WS_APP" 'round(w["layout"]["tile_size"][0])'; }
OUT_W=$(nmsg -j outputs | python3 -c 'import json,sys; print(round(next(iter(json.load(sys.stdin).values()))["logical"]["width"]))')
floats_is() { [[ $(wjs "$WS_APP" 'w["is_floating"]') == "$1" ]]; }
wide_is() { # FELD (ws_w|ws_tile) VOLL(1|0)
    local v
    v=$($1)
    if (($2)); then ((v >= OUT_W - 2)); else ((v > 0 && v < OUT_W * 3 / 4)); fi
}
W0=$(ws_w)
focus dictate-test-target
pal_run qqfloat || bad "palette (floating): run"
check "palette (floating): the window floats" nested_wait 5 floats_is True
settle
pal_run qqfloat || bad "palette (floating back): run"
check "palette (floating): back in the layout" nested_wait 5 floats_is False
settle
focus dictate-test-target
pal_run qqfull || bad "palette (fullscreen): run"
check "palette (fullscreen): the window is as wide as the output ($OUT_W)" nested_wait 5 wide_is ws_w 1
settle
pal_run qqfull || bad "palette (fullscreen back): run"
check "palette (fullscreen): back to its width ($W0)" nested_wait 5 wide_is ws_w 0
settle
focus dictate-test-target
pal_run qqmax || bad "palette (maximized): run"
check "palette (maximized): the tile fills the width" nested_wait 5 wide_is ws_tile 1
settle
pal_run qqmax || bad "palette (maximized back): run"
check "palette (maximized): back to its width" nested_wait 5 wide_is ws_tile 0
check "palette (toggles): three toggles each way done" test "$(pal_logged 'toggle_window done')" -ge 6
settle
kill -TERM -- "-$WS_PID" 2>/dev/null
nested_wait 5 bash -c "! nmsg -j windows | grep -q '\"$WS_APP\"'" || bad "palette (toggles): window closed"

# --- P26. close_window (confirm): erst nach dem zweiten Return, nur das Ziel aus dem Kandidaten ---------------------------
CLOSE_APP=dictate-test-close # PAL_CLOSE_APP des Mocks
nested_spawn palclose python3 "$HERE/target.py" text "$CLOSE_APP"
nested_wait 10 has_win "$CLOSE_APP" || bad "palette (close): window"
focus dictate-test-target || bad "palette (close): focus the target"
close_not_yet() { has_win "$CLOSE_APP"; }
c0=$(pal_chosen_n close_window)
PAL_BEFORE_CONFIRM=close_not_yet pal_run qqclose confirm || bad "palette (close): run (window still there before the second Return)"
check "palette (close): the window is gone" nested_wait 5 bash -c "! nmsg -j windows | grep -q '\"$CLOSE_APP\"'"
check "palette (close): the target stays" has_win dictate-test-target
check "palette (close): pal_chosen close_window" nested_wait 5 more "pal_chosen close_window" "$c0"
settle
# ohne das Fenster liefert der Mock keinen Kandidaten (kein Ersatzziel): Return führt nichts aus
m0=$(count 'pal_target_missing close_window') c0=$(pal_chosen_n close_window)
pal_open || bad "palette (close gone): open"
pal_type qqclose || bad "palette (close gone): type"
check "palette (close gone): no candidate without the window" more 'pal_target_missing close_window' "$m0"
pal_close || bad "palette (close gone): close"
check "palette (close gone): nothing closed" test "$(pal_chosen_n close_window)" -eq "$c0" -a -n "$(win_id dictate-test-target)"
settle

# --- P27. screenshot (full, window, area): niri nimmt das Bild ohne das Panel; nie auf die Platte (screenshot-path null) --------
# Ziel der Bilder ist nur die Zwischenablage des verschachtelten niri. Vor jeder Aufnahme: die Config sagt `screenshot-path
# null` (die Vorgabe schriebe nach ~/Pictures), CLIPBOARD trägt Text (so zeigt image/png die neue Aufnahme).
# full und area (Entscheidung 17): das Panel wird erst durchsichtig, dann nimmt niri auf, erst danach schließt es.
touch "$D/shots.mark"
shots_home() { find "$HOME/Pictures" -maxdepth 2 -newer "$D/shots.mark" -name '*.png' 2>/dev/null | wc -l; }
# Reihenfolge im Log nach Zeile N: erst durchsichtig (Deckkraft, Titel der Abgangsregel), dann ausgeführt, erst dann
# verborgen. Gegenprobe zum Stand vor D3: dort stand „panel hidden“ vor „palette: run screenshot“, „turning transparent“
# fehlte. Ob GTK den Frame vor der Frist von 300 ms meldete, prüft der Test nicht: im verschachtelten niri kommt der
# Frame-Rückruf nach 330–430 ms, mit Animationen nach ~700 ms (gemessen in D3); dann führt die Frist aus
vanish_order() {
    tail -n +"$(($1 + 1))" "$UI_LOG" | python3 -c 'import sys
lines = sys.stdin.read().splitlines()
def at(s, start=0):
    return next((i for i in range(start, len(lines)) if s in lines[i]), None)
t = at("panel: turning transparent"); r = at("palette: run screenshot"); h = at("panel hidden")
sys.exit(0 if None not in (t, r, h) and t < r < h else 1)'
}
no_disk_path() { grep -qx 'screenshot-path null' "$NESTED_CONFIG_FILE"; }
mkdir -p "$D/shotclip"
printf 'text/plain;charset=utf-8\n' >"$D/shotclip/types.txt"
printf 'vor der Aufnahme' >"$D/shotclip/0.bin"
SHOT_FDS=()
clip_text() { # CLIPBOARD auf Text setzen (eigener Anbieter)
    local n=${#SHOT_FDS[@]} fd
    mkfifo "$D/shotclip-$n.in"
    exec {fd}<>"$D/shotclip-$n.in"
    SHOT_FDS+=("$fd")
    nested_spawn "shotclip$n" "$CLIP" offer "$D/shotclip" <"$D/shotclip-$n.in"
    nested_wait 5 bash -c "WAYLAND_DISPLAY='$NESTED_NIRI_WL' '$CLIP' types 2>/dev/null | grep -q '^text/plain'"
}
clip_png() { clip_types | grep -qw 'image/png'; }
png_size() { python3 -c 'import sys; from PIL import Image; print("%dx%d" % Image.open(sys.argv[1]).size)' "$1"; }
# LUM X Y (logische Pixel): Helligkeit im Bild
png_lum() { python3 -c 'import sys; from PIL import Image
im=Image.open(sys.argv[1]).convert("RGB"); s=float(sys.argv[4]); p=im.getpixel((round(float(sys.argv[2])*s), round(float(sys.argv[3])*s)))
print(round(0.3*p[0]+0.59*p[1]+0.11*p[2]))' "$1" "$2" "$3" "$NESTED_SCALE"; }
# Pixel, die sich zwischen REF und SHOT um mehr als 12 unterscheiden, im Panel samt 40 Pixeln darum (logische Lage X Y W H;
# dort lägen Fokusring, Rahmen und Schatten)
png_diff() { python3 -c 'import sys; from PIL import Image, ImageChops
a, b = (Image.open(f).convert("RGB") for f in sys.argv[1:3]); x, y, w, h, s = (float(v) for v in sys.argv[3:8]); m = 40 * s
box = tuple(round(v) for v in (x * s - m, y * s - m, (x + w) * s + m, (y + h) * s + m))
print(sum(1 for v in ImageChops.difference(a, b).convert("L").crop(box).getdata() if v > 12))' "$@" "$NESTED_SCALE"; }
# Nach einem Abgang ist das Panel beim nächsten Öffnen wieder deckend (Deckkraft und Titel aus `show`): dunkel im Bild
opaque_again() {
    pal_open || return 1
    clip_text || return 1
    nmsg action screenshot-screen
    nested_wait 5 clip_png && clip_get image/png >"$D/shot-again.png"
    local l
    l=$(png_lum "$D/shot-again.png" "$PX" "$PY")
    pal_close
    test "${l:-255}" -lt 110
}
h0=$(shots_home)
focus dictate-test-target || bad "palette (screenshot): focus the target"
# Gegenprobe: niri nimmt das offene Panel mit (dunkel im linken Viertel, die Mitte trifft den Mauszeiger); Lage aus niri
pal_open || bad "palette (screenshot control): open"
read -r PX PY < <(panel_field '"%.0f %.0f" % (w["layout"]["tile_pos_in_workspace_view"][0] + w["layout"]["tile_size"][0] / 4, w["layout"]["tile_pos_in_workspace_view"][1] + w["layout"]["tile_size"][1] / 2)')
read -r RX RY RW RH < <(panel_field '"%.1f %.1f %.1f %.1f" % (*w["layout"]["tile_pos_in_workspace_view"], *w["layout"]["tile_size"])')
no_disk_path || bad "palette (screenshot control): config without screenshot-path null"
clip_text || bad "palette (screenshot control): text in CLIPBOARD"
nmsg action screenshot-screen
nested_wait 5 clip_png && clip_get image/png >"$D/shot-control.png"
l=$(png_lum "$D/shot-control.png" "$PX" "$PY")
check "palette (screenshot control): the open panel is in a screenshot (dark at $PX,$PY: $l)" test "${l:-255}" -lt 110
pal_close || bad "palette (screenshot control): close"
# Bild ohne Panel (Animationen sind aus): Vergleich für full
clip_text || bad "palette (screenshot reference): text in CLIPBOARD"
nmsg action screenshot-screen
nested_wait 5 clip_png && clip_get image/png >"$D/shot-ref.png"
n=$(png_diff "$D/shot-ref.png" "$D/shot-control.png" "$RX" "$RY" "$RW" "$RH")
check "palette (screenshot reference): differs from the control where the panel was ($n px)" test "${n:-0}" -gt 100000
# full: das Panel ist fort, bevor niri aufnimmt
no_disk_path || bad "palette (screenshot full): config without screenshot-path null"
clip_text || bad "palette (screenshot full): text in CLIPBOARD"
c0=$(pal_chosen_n screenshot)
n0=$(wc -l <"$UI_LOG")
pal_run qqshotfull || bad "palette (screenshot full): run"
check "palette (screenshot full): image in CLIPBOARD" nested_wait 5 clip_png
check "palette (screenshot full): transparent, then taken, then closed" vanish_order "$n0"
clip_get image/png >"$D/shot-full.png"
check "palette (screenshot full): the whole output ($(png_size "$D/shot-full.png"))" test "$(png_size "$D/shot-full.png")" = "$(nmsg -j outputs | python3 -c 'import json,sys; o=next(iter(json.load(sys.stdin).values())); m=o["modes"][o["current_mode"]]; print("%dx%d" % (m["width"], m["height"]))')"
l=$(png_lum "$D/shot-full.png" "$PX" "$PY")
check "palette (screenshot full): the panel is not in the image (light at $PX,$PY: $l)" test "${l:-0}" -gt 150
# ohne die Abgangsregel der KDL füllte niri die Fläche in Ringfarbe (Helligkeit 167) und zeichnete Ring und Schatten
n=$(png_diff "$D/shot-ref.png" "$D/shot-full.png" "$RX" "$RY" "$RW" "$RH")
check "palette (screenshot full): same as without the panel, no ring or fill ($n px differ)" test "${n:-1}" -le 200
check "palette (screenshot full): pal_chosen screenshot" nested_wait 5 more "pal_chosen screenshot" "$c0"
settle
check "palette (screenshot full): opaque again at the next opening" opaque_again
# window: das Fenster vor dem Öffnen (der Daemon schickt kein Ziel)
no_disk_path || bad "palette (screenshot window): config without screenshot-path null"
clip_text || bad "palette (screenshot window): text in CLIPBOARD"
TW=$(wjs dictate-test-target '"%dx%d" % tuple(round(v * '"$NESTED_SCALE"') for v in w["layout"]["window_size"])')
n0=$(wc -l <"$UI_LOG")
pal_run qqshotwin || bad "palette (screenshot window): run"
check "palette (screenshot window): closed first, never transparent" bash -c "! tail -n +$((n0 + 1)) '$UI_LOG' | grep -q 'panel: turning transparent'"
check "palette (screenshot window): image in CLIPBOARD" nested_wait 5 clip_png
clip_get image/png >"$D/shot-window.png"
check "palette (screenshot window): the size of the window before opening ($(png_size "$D/shot-window.png") ~ $TW)" python3 -c 'import sys
a=[int(x) for x in sys.argv[1].split("x")]; b=[int(x) for x in sys.argv[2].split("x")]
sys.exit(0 if all(abs(x-y)<=2 for x,y in zip(a,b)) else 1)' "$(png_size "$D/shot-window.png")" "$TW"
settle
# area: niris Auswahl; Return bestätigt, danach kommen Tasten wieder an
no_disk_path || bad "palette (screenshot area): config without screenshot-path null"
clip_text || bad "palette (screenshot area): text in CLIPBOARD"
d0=$(pal_logged "screenshot done")
n0=$(wc -l <"$UI_LOG")
pal_run qqshotarea || bad "palette (screenshot area): run"
check "palette (screenshot area): done in the log" nested_wait 3 more_pal_logged "screenshot done" "$d0"
check "palette (screenshot area): transparent, then opened, then closed" vanish_order "$n0"
sleep 0.5
check "palette (screenshot area): nothing taken before confirming" bash -c "! WAYLAND_DISPLAY='$NESTED_NIRI_WL' '$CLIP' types 2>/dev/null | grep -qw image/png"
nflush
nested_shot "$D/palette-shot-area.png" || true
key return
check "palette (screenshot area): Return takes the area" nested_wait 5 clip_png
settle
check "palette (screenshot area): keys reach the palette again" pal_open
pal_close
# area mit Esc abgebrochen: nichts in CLIPBOARD, danach kommen Tasten wieder an (gewählt zählt es trotzdem, L38)
clip_text || bad "palette (screenshot area, Esc): text in CLIPBOARD"
pal_run qqshotarea || bad "palette (screenshot area, Esc): run"
sleep 0.5
key escape
sleep 0.5
check "palette (screenshot area, Esc): nothing taken" bash -c "! WAYLAND_DISPLAY='$NESTED_NIRI_WL' '$CLIP' types 2>/dev/null | grep -qw image/png"
check "palette (screenshot area, Esc): keys reach the palette again" pal_open
pal_close
check "palette (screenshot area): opaque again at the next opening" opaque_again
check "palette (screenshot): nothing written to ~/Pictures" test "$(shots_home)" -eq "$h0"
for fd in "${SHOT_FDS[@]}"; do exec {fd}>&-; done
settle

# --- P12. Keine Fakes mehr am Laufen (über niri `Spawn` gestartete sind keine Kinder dieses Skripts) ---------------------
check "palette: no fake program left running" nested_wait 3 bash -c "! pgrep -f '[${FAKES:0:1}]${FAKES:1}/' >/dev/null"
grep -E "palette: [a-z_]+ (done|failed|handoff)|recording for the|final: .*palette" "$UI_LOG" | sed 's/^.*Z /info /' | tail -12
