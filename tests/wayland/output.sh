# Copyright (C) 2026 Daniel Kurdoghlian
# Pushing Pixels, Vienna <https://pushingpixels.at>
# SPDX-License-Identifier: GPL-3.0-or-later
# Prüfungen der Textausgabe von dictate-ui (Block W2), eingebunden von tests/wayland/run.sh nach den Prüfungen aus W1
# (laufende Oberfläche, Mock-Daemon, verschachteltes niri mit xwayland-satellite). Nur Testtexte; ins Log von
# dictate-ui darf keiner davon gelangen (Prüfung am Ende von run.sh).
#
# Ziele: GTK-Textfeld mit Eingabemethode (T3), dasselbe Feld als Ausnahme `typing.keyboard_only_app_ids` (T2 bzw.
# Einfügen), Alacritty (T3 und Return), xterm über XWayland (T2, Terminal). Zwischenablage über `nested-clip`.

CLIP="$HERE/nested/target/debug/nested-clip"
TEXT='Grüße, Maß: äöü ÄÖÜ ß € – „ok“ :,-.;!?'
T3APP=dictate-test-text
KBAPP=dictate-test-kbd

mock_cmd() { # JSON: eine Zeile an den Mock, Antwort verwerfen
    python3 - "$NESTED_RT/dictate/control.sock" "$1" <<'PY'
import socket, sys
s = socket.socket(socket.AF_UNIX)
s.connect(sys.argv[1])
s.sendall(sys.argv[2].encode() + b"\n")
s.recv(4096)
PY
}
final_json() { # TEXT [KEYS…]: JSON für test_final
    python3 -c 'import json,sys; print(json.dumps({"cmd": "test_final", "text": sys.argv[1], "keys": sys.argv[2:]}))' "$@"
}
send_final() { mock_cmd "$(final_json "$@")"; }
# Config des Mocks setzen und dictate-ui neu laden lassen (Event terminals → get_config)
set_config() { # JSON
    local n
    n=$(grep -c 'output: paste' "$UI_LOG")
    echo "$1" >"$D/config.json"
    mock_cmd '{"cmd":"test_terminals"}'
    nested_wait 3 bash -c "(( \$(grep -c 'output: paste' '$UI_LOG') > $n ))"
}
win_id() { nmsg -j windows | python3 -c 'import json,sys; a=sys.argv[1]; print(next((w["id"] for w in json.load(sys.stdin) if w["app_id"]==a or w["title"]==a), ""))' "$1"; }
has_win() { [[ -n $(win_id "$1") ]]; }
focused_is() { [[ $(nmsg -j focused-window | python3 -c 'import json,sys; w=json.load(sys.stdin) or {}; print(w.get("id"))') == "$1" ]]; }
focus() { # APP_ID oder Titel
    local id
    id=$(win_id "$1")
    [[ -n $id ]] && nmsg action focus-window --id "$id" && nested_wait 3 focused_is "$id"
}
# letzter Inhalt eines Textfensters (Zeile `text <JSON>` im Log des Fensters)
text_of() { python3 -c 'import json,sys
t=""
for l in open(sys.argv[1], encoding="utf-8"):
    if l.startswith("text "): t=json.loads(l[5:])
sys.stdout.write(t)' "$NESTED_DIR/$1.log"; }
text_is() { [[ "$(text_of "$1")" == "$2" ]]; }
# Zeichen des letzten Inhalts (unabhängig von der Locale, ohne dass die Shell Zeilenumbrüche am Ende abschneidet)
tlen() { python3 -c 'import json,sys
t=""
for l in open(sys.argv[1], encoding="utf-8"):
    if l.startswith("text "): t=json.loads(l[5:])
print(len(t))' "$NESTED_DIR/$1.log"; }
# Was seit Zeichen N dazukam, enthält keins der Zeichen aus VERBOTEN
added_without() { # APP N VERBOTEN
    python3 - "$NESTED_DIR/$1.log" "$2" "$3" <<'PY2'
import json, sys
t = ""
for l in open(sys.argv[1], encoding="utf-8"):
    if l.startswith("text "):
        t = json.loads(l[5:])
added = t[int(sys.argv[2]):]
sys.exit(1 if any(c in added for c in sys.argv[3]) else 0)
PY2
}
bytes_are() { # DATEI ERWARTET (printf-Format)
    python3 - "$1" "$2" <<'PY'
import sys
want = sys.argv[2].encode().decode("unicode_escape").encode("latin-1")
try:
    got = open(sys.argv[1], "rb").read()
except OSError:
    got = b""
sys.exit(0 if got == want else 1)
PY
}
routes() { grep -c "route $1" "$UI_LOG"; }
wait_route() { # ROUTE ANZAHL_VORHER
    nested_wait 5 bash -c "(( \$(grep -c 'route $1' '$UI_LOG') > $2 ))"
}
done_count() { grep -cE 'output job [0-9]+: [0-9]+ chars, [0-9]+ keys: ' "$UI_LOG"; }
wait_done() { nested_wait "${2:-10}" bash -c "(( \$(grep -cE 'output job [0-9]+: [0-9]+ chars, [0-9]+ keys: ' '$UI_LOG') > $1 ))"; }

cat >"$D/rawcat.py" <<'EOF'
import os, sys, tty
tty.setraw(0)
with open(sys.argv[1], "ab", 0) as f:
    while True:
        b = os.read(0, 4096)
        if not b:
            break
        f.write(b)
EOF

echo 0 >"$KEYS"
echo '{"typing": {"output": "paste", "keyboard_only_app_ids": ["dictate-test-kbd"]}, "actions": {"terminal_classes": ["Alacritty", "XTerm", "xterm"]}}' >"$D/config.json"
mock_cmd '{"cmd":"test_terminals"}'
nested_spawn t3 python3 "$HERE/target.py" text "$T3APP"
nested_spawn kb python3 "$HERE/target.py" text "$KBAPP"
nested_wait 10 has_win "$T3APP" || bad "text window $T3APP"
nested_wait 10 has_win "$KBAPP" || bad "text window $KBAPP"
nmsg -j windows >"$D/windows-w2.json"

# --- 11. T3: GTK-Feld mit Eingabemethode, Umlaute, zweite Zeile (Shift+Return über T2), langer Text in Stücken ---
focus "$T3APP" || bad "focus $T3APP"
n=$(routes input-method)
d0=$(done_count)
send_final "$TEXT
Zweite Zeile"
check "T3: route input method" wait_route input-method "$n"
check "T3: GTK field gets the exact text with a newline" nested_wait 5 text_is t3 "$TEXT
Zweite Zeile"
wait_done "$d0"
check "T3: input method destroyed after the output" nested_wait 3 bash -c "grep -q 'input method destroyed' '$UI_LOG'"
grep -o 'settled after .* niri [0-9]* ms)' "$UI_LOG" | head -n 2 | sed 's/^/info T3 /'
LONG=$(python3 -c 'print("äöü " * 715 + "Ende", end="")')
before=$(text_of t3)
d0=$(done_count)
send_final "$LONG"
check "T3: 5000-byte text arrives exactly (several commits)" nested_wait 8 text_is t3 "$before$LONG"
wait_done "$d0"

# --- 12. T2 im GTK-Feld (Ausnahme keyboard_only, typing.output = type): exakte Zeichen -----------------------------
set_config '{"typing": {"output": "type", "keyboard_only_app_ids": ["dictate-test-kbd"]}, "actions": {"terminal_classes": ["Alacritty", "XTerm", "xterm"]}}' || bad "config reload (type)"
focus "$KBAPP" || bad "focus $KBAPP"
n=$(routes keyboard)
d0=$(done_count)
send_final "$TEXT"
check "T2: route keyboard for a keyboard-only app" wait_route keyboard "$n"
check "T2: GTK field gets the exact text (umlauts, ß, €, quotes)" nested_wait 8 text_is kb "$TEXT"
wait_done "$d0"
# BackSpace aus final.keys, andere Tasten verworfen
before=$(text_of kb)
d0=$(done_count)
send_final "" BackSpace Return ctrl+w BackSpace
wait_done "$d0"
check "T2: only BackSpace from final.keys (two removed)" nested_wait 3 text_is kb "${before%??}"

# --- 13. Modifikator gehalten: nichts wird geschrieben, bis er los ist; Positivkontrolle Mod+Q --------------------
before=$(text_of kb)
echo "0 mod" >"$KEYS"
nkeys down:alt
d0=$(done_count)
send_final "qq"
sleep 0.8
check "mods: nothing typed while a modifier is held" text_is kb "$before"
check "mods: no shortcut fired while waiting" test ! -e "$MARK/mod-q"
nkeys up:alt
nflush
echo 0 >"$KEYS"
check "mods: typed after the release" nested_wait 5 text_is kb "${before}qq"
wait_done "$d0"
# Physisch gehaltenes Alt, Tastenzustand sagt „los“ (wie ein nicht erkanntes Gerät): die virtuelle Tastatur erbt den
# Modifikator nicht und löst kein Kürzel aus (S2)
before=$(text_of kb)
nkeys down:alt
d0=$(done_count)
send_final "q"
check "mods: virtual key with physically held Alt arrives plain" nested_wait 5 text_is kb "${before}q"
wait_done "$d0"
nkeys up:alt
nflush
check "mods: no shortcut from a virtual key with Alt held" test ! -e "$MARK/mod-q"
check "mods: target saw no modified q" bash -c "! grep -q '^key q ' '$NESTED_DIR/kb.log'"
nkeys alt+q
nflush
check "mods: positive control, physical Mod+Q fires the shortcut" nested_wait 3 test -e "$MARK/mod-q"
focus "$KBAPP"

# --- 14. Abbruch mitten im Tippen: Warteschlange leer, keine Taste bleibt hängen --------------------------------
set_config '{"typing": {"output": "type", "delay_ms": 40, "keyboard_only_app_ids": ["dictate-test-kbd"]}, "actions": {"terminal_classes": ["Alacritty", "XTerm", "xterm"]}}' || bad "config reload (slow)"
b0=$(tlen kb)
d0=$(done_count)
send_final "$(python3 -c 'print("abcdefghij" * 15, end="")')"
send_final "zweiter Auftrag in der Warteschlange"
grown10() { (($(tlen kb) >= b0 + 10)); }
nested_wait 5 grown10
press ctrl+alt+e
wait_done "$d0"
sleep 0.3
l1=$(tlen kb)
sleep 1.2
l2=$(tlen kb)
check "cancel: typing stops ($((l1 - b0)) of 150 chars)" test "$l1" -lt $((b0 + 150))
check "cancel: no key repeats after the cancel ($l1 → $l2)" test "$l1" -eq "$l2"
check "cancel: waiting job dropped" added_without kb "$b0" zw
check "cancel: logged as stopped" grep -q 'keys: Stopped' "$UI_LOG"

# --- 14b. Fokuswechsel mitten im Tippen: vor dem Zeilenumbruch gestoppt, nichts danach im anderen Fenster ----------
focus "$KBAPP"
b0=$(tlen kb)
t0=$(tlen t3)
d0=$(done_count)
send_final "$(python3 -c 'print("x" * 40 + "\n" + "yyyy", end="")')"
grown() { (($(tlen kb) >= b0 + 5)); }
nested_wait 5 grown
focus "$T3APP"
wait_done "$d0"
check "focus moved: output stopped with a notice" grep -q 'focused window changed, output stopped' "$UI_LOG"
check "focus moved: no newline or later text in the other window" added_without t3 "$t0" $'y\n'
check "focus moved: no newline or later text in the first window" added_without kb "$b0" $'y\n'
focus "$KBAPP"

# --- 15. Spätes final nach Abbruch einer Aufnahme wird verworfen ----------------------------------------------
set_config '{"typing": {"output": "type", "keyboard_only_app_ids": ["dictate-test-kbd"]}, "actions": {"terminal_classes": ["Alacritty", "XTerm", "xterm"]}}' || bad "config reload"
before=$(text_of kb)
touch "$D/late_final"
c0=$(count cancel)
s0=$(count start)
echo 1 >"$KEYS"
press alt+e
nested_wait 3 at_least start $((s0 + 1))
press ctrl+alt+e
check "late final: cancel sent" nested_wait 3 at_least cancel $((c0 + 1))
echo 0 >"$KEYS"
check "late final: discarded" nested_wait 3 grep -q 'final: [0-9]* chars, 0 keys, discarded' "$UI_LOG"
rm -f "$D/late_final"
sleep 0.5
check "late final: nothing typed" text_is kb "$before"
d0=$(done_count)
send_final "danach"
check "late final: the next final is typed again" nested_wait 5 text_is kb "${before}danach"
wait_done "$d0"
check "start carries the app_id of the focused window" grep -q " app $KBAPP\$" "$MOCK_LOG"

# --- 16. Einfügen (keyboard_only + paste): alle Typen samt fremdem Typ und 5 MB zurück, Verlauf mit Hinweis ------
set_config '{"typing": {"output": "paste", "keyboard_only_app_ids": ["dictate-test-kbd"]}, "actions": {"terminal_classes": ["Alacritty", "XTerm", "xterm"]}}' || bad "config reload (paste)"
mkdir -p "$D/clip-in" "$D/clip-out" "$D/clip-secret"
python3 - "$D/clip-in" "$D/clip-secret" <<'PY'
import os, sys
d, s = sys.argv[1], sys.argv[2]
types = ["text/plain;charset=utf-8", "text/plain", "UTF8_STRING", "text/html", "image/png", "application/x-dictate-test"]
data = [b"alte Zwischenablage", b"alte Zwischenablage", b"alte Zwischenablage", b"<b>alt</b>", os.urandom(5 * 1024 * 1024), os.urandom(64 * 1024)]
open(os.path.join(d, "types.txt"), "w").write("\n".join(types) + "\n")
for i, b in enumerate(data):
    open(os.path.join(d, f"{i}.bin"), "wb").write(b)
types = ["text/plain;charset=utf-8", "text/plain", "x-kde-passwordManagerHint"]
data = [b"Passwort123", b"Passwort123", b"secret"]
open(os.path.join(s, "types.txt"), "w").write("\n".join(types) + "\n")
for i, b in enumerate(data):
    open(os.path.join(s, f"{i}.bin"), "wb").write(b)
PY
mkfifo "$D/offer.in"
exec {OFD}<>"$D/offer.in"
nested_spawn offer "$CLIP" offer "$D/clip-in" <"$D/offer.in"
nested_wait 5 grep -q 'offering 6 types' "$NESTED_DIR/offer.log" || bad "clipboard offered"
nested_spawn hist "$CLIP" watch respect </dev/null
HIST_PID=$NESTED_LAST_PID
# Gieriger Verlauf (holt jeden Text sofort, auch mit Hinweis): seine Abholung vor dem Ctrl+V darf nicht als Bestätigung
# zählen (sonst würde zu früh zurückgelegt und das Ziel bekäme den alten Inhalt)
nested_spawn greedy "$CLIP" watch greedy </dev/null
GREEDY_PID=$NESTED_LAST_PID
sleep 0.3
focus "$KBAPP"
before=$(text_of kb)
n=$(routes paste)
d0=$(done_count)
send_final "Eingefügt $TEXT
zweite Zeile"
check "paste: route paste" wait_route paste "$n"
check "paste: GTK field gets the text (with the newline)" nested_wait 8 text_is kb "${before}Eingefügt $TEXT
zweite Zeile"
wait_done "$d0"
check "paste: confirmed" bash -c "grep -E 'output job [0-9]+: .*keys: Written' '$UI_LOG' | tail -n 1 | grep -q Written"
check "paste: owner lost the selection while pasting" grep -q cancelled "$NESTED_DIR/offer.log"
check "paste: history skipped the dictated text (hint secret)" grep -q 'skipped (secret)' "$NESTED_DIR/hist.log"
WAYLAND_DISPLAY="$NESTED_NIRI_WL" "$CLIP" save "$D/clip-out" 2>"$D/clip-save.log"
check "paste: all 6 types restored" cmp -s "$D/clip-in/types.txt" "$D/clip-out/types.txt"
same=1
for i in 0 1 2 3 4 5; do cmp -s "$D/clip-in/$i.bin" "$D/clip-out/$i.bin" || same=0; done
check "paste: every restored type byte-identical (5 MB image, unknown type)" test "$same" -eq 1
check "paste: the greedy history fetched the text too (and the target still got it)" grep -q 'stored text/plain' "$NESTED_DIR/greedy.log"
kill -TERM "$GREEDY_PID" 2>/dev/null
exec {OFD}>&-

# --- 16a. Einfügen in ein XWayland-Programm (eigentlicher Fall des Einfügens: keine Eingabemethode, kein Terminal) -
XDISP="$(sed 's/\x1b\[[0-9;]*m//g' "$NESTED_DIR/niri.log" | grep -oE 'listening on X11 socket: :[0-9]+' | grep -oE ':[0-9]+$' | head -1)"
if [[ -n $XDISP && $XDISP != :0 ]]; then
    DISPLAY="$XDISP" GDK_BACKEND=x11 nested_spawn x11 python3 "$HERE/target.py" text dictate-test-x11
    nested_wait 30 has_win dictate-test-x11 || bad "x11 text window"
    sleep 0.5
    focus dictate-test-x11
    nmsg -j focused-window >"$D/x11-window.json"
    n=$(routes paste)
    d0=$(done_count)
    send_final "Über X eingefügt"
    check "x11: route paste (no input method, not a terminal)" wait_route paste "$n"
    check "x11: XWayland field gets the pasted text" nested_wait 8 text_is x11 "Über X eingefügt"
    wait_done "$d0" 12
    check "x11: paste confirmed (fetch after the Ctrl+V seen)" bash -c "grep -E 'output job [0-9]+: .*keys: ' '$UI_LOG' | tail -n 1 | grep -q 'keys: Written'"
    focus "$KBAPP"
else
    bad "x11 paste skipped: no safe X display (${XDISP:-none})"
fi

# --- 16b. Auswahl wechselt während des Sicherns (Passwort kopiert): nie übernehmen, tippen ------------------------
mkfifo "$D/slow.in"
exec {LFD}<>"$D/slow.in"
CLIP_DELAY_MS=250 nested_spawn slow "$CLIP" offer "$D/clip-in" <"$D/slow.in"
nested_wait 5 grep -q 'offering 6 types' "$NESTED_DIR/slow.log" || bad "slow clipboard offered"
sleep 0.3
focus "$KBAPP"
before=$(text_of kb)
d0=$(done_count)
send_final "Während des Sicherns"
# text/html holt nur dictate-ui beim Sichern (xwayland-satellite spiegelt jede neue Auswahl sofort nach X, holt aber
# nur Text): danach wechselt die Auswahl, während dictate-ui noch die übrigen Typen liest
nested_wait 5 grep -q 'send #[0-9]* text/html' "$NESTED_DIR/slow.log" || bad "dictate-ui started saving"
mkfifo "$D/secret.in"
exec {SFD}<>"$D/secret.in"
nested_spawn secret "$CLIP" offer "$D/clip-secret" <"$D/secret.in"
check "changed: text typed instead" nested_wait 8 text_is kb "${before}Während des Sicherns"
wait_done "$d0"
check "changed: reason in the log" grep -q 'not pasting (clipboard changed while saving)' "$UI_LOG"
exec {LFD}>&-

# --- 17. Passwort in der Zwischenablage: nicht einfügen, tippen; Passwort nie gelesen, Besitzer behält sie -------
# (die Passwort-Auswahl aus 16b gilt noch)
nested_wait 5 grep -q 'offering 3 types' "$NESTED_DIR/secret.log" || bad "secret clipboard offered"
sleep 0.3
focus "$KBAPP"
before=$(text_of kb)
d0=$(done_count)
send_final "Neben dem Passwort"
check "secret: text typed instead" nested_wait 8 text_is kb "${before}Neben dem Passwort"
wait_done "$d0"
check "secret: reason in the log" grep -q 'not pasting (clipboard holds a password)' "$UI_LOG"
check "secret: owner keeps the selection through 16b and 17 (no cancelled)" bash -c "! grep -q cancelled '$NESTED_DIR/secret.log'"
check "secret: only the hint was read, never the password" bash -c "! grep -E 'send #[0-9]+ ' '$NESTED_DIR/secret.log' | grep -qv 'x-kde-passwordManagerHint'"
exec {SFD}>&-
kill -TERM "$HIST_PID" 2>/dev/null

# --- 18. Alacritty: T3 und Return (Terminal) in der richtigen Reihenfolge --------------------------------------
if command -v alacritty >/dev/null; then
    : >"$D/alacritty.bin"
    nested_spawn alacritty alacritty -e python3 -I "$D/rawcat.py" "$D/alacritty.bin" </dev/null
    nested_wait 10 has_win Alacritty || bad "alacritty window"
    sleep 0.5
    focus Alacritty
    n=$(routes input-method)
    d0=$(done_count)
    send_final "echo eins
echo zwei"
    check "alacritty: route input method" wait_route input-method "$n"
    check "alacritty: text and Return in order" nested_wait 5 bytes_are "$D/alacritty.bin" 'echo eins\recho zwei'
    wait_done "$d0"
else
    bad "alacritty not installed"
fi

# --- 18b. Chromium mit T2 (Ausnahmeliste, typing.output = type): Text, der alle 48 Zeichenplätze und einen zweiten
# Abschnitt füllt. Chromium deutete Code 14 als Rücktaste (S2); jede Taste auf einem Sondercode verlöre hier Zeichen
if command -v chromium >/dev/null; then
    mkdir -p "$D/chr"
    cat >"$D/chr/page.html" <<'EOF'
<!doctype html><meta charset="utf-8"><title>READY</title>
<textarea id="t" autofocus style="width:90vw;height:60vh" oninput="document.title='T:'+JSON.stringify(this.value)"></textarea>
<script>document.getElementById('t').focus()</script>
EOF
    nested_spawn chromium chromium --user-data-dir="$D/chr/profile" --ozone-platform=wayland --no-first-run \
        --no-default-browser-check --disable-background-networking --disable-component-update --disable-sync \
        --password-store=basic --disable-gpu --app="file://$D/chr/page.html" </dev/null
    chr_id() { nmsg -j windows | python3 -c 'import json,sys; print(next((w["id"] for w in json.load(sys.stdin) if "chrom" in (w["app_id"] or "").lower()), ""))'; }
    chr_title() { nmsg -j windows | python3 -c 'import json,sys; print(next((w["title"] for w in json.load(sys.stdin) if "chrom" in (w["app_id"] or "").lower()), ""), end="")'; }
    chr_has() { [[ -n $(chr_id) ]]; }
    chr_ready() { [[ $(chr_title) == READY* ]]; }
    if nested_wait 90 chr_has && nested_wait 30 chr_ready; then
        CHRAPP=$(nmsg -j windows | python3 -c 'import json,sys; print(next(w["app_id"] for w in json.load(sys.stdin) if "chrom" in (w["app_id"] or "").lower()))')
        set_config "{\"typing\": {\"output\": \"type\", \"delay_ms\": 4, \"keyboard_only_app_ids\": [\"dictate-test-kbd\", \"$CHRAPP\"]}, \"actions\": {\"terminal_classes\": [\"Alacritty\", \"XTerm\", \"xterm\"]}}" || bad "config reload (chromium)"
        nmsg action focus-window --id "$(chr_id)"
        sleep 0.5
        CHRTEXT='qwertzuiopüasdfghjklöäyxcvbnmQWERTZUIOPÜASDFGHJKLÖÄYXCVBNM 1234567890 ß:,-.;!?/()=+*#<>|@€„“–'
        want="T:$(python3 -c 'import json,sys; print(json.dumps(sys.argv[1], ensure_ascii=False), end="")' "$CHRTEXT")"
        chr_ok() { [[ $(chr_title) == "$want" ]]; }
        n=$(routes keyboard)
        d0=$(done_count)
        send_final "$CHRTEXT"
        check "chromium: route keyboard (keyboard-only app)" wait_route keyboard "$n"
        wait_done "$d0" 20
        check "chromium: every character on every slot arrives (no special code)" nested_wait 5 chr_ok
        chr_title >"$D/chromium-title.txt"
    else
        bad "chromium window not ready"
    fi
else
    bad "chromium not installed"
fi

# --- 19. xterm über XWayland: Terminal ohne Eingabemethode → T2, Umlaute exakt, Return -------------------------
XDISP="$(sed 's/\x1b\[[0-9;]*m//g' "$NESTED_DIR/niri.log" | grep -oE 'listening on X11 socket: :[0-9]+' | grep -oE ':[0-9]+$' | head -1)"
if [[ -n $XDISP && $XDISP != :0 ]] && command -v xterm >/dev/null; then
    : >"$D/xterm.bin"
    DISPLAY="$XDISP" LC_ALL=en_US.UTF-8 nested_spawn xterm xterm -u8 -T dictate-xterm -e python3 -I "$D/rawcat.py" "$D/xterm.bin" </dev/null
    nested_wait 30 has_win dictate-xterm || bad "xterm window"
    sleep 0.5
    focus dictate-xterm
    nmsg -j focused-window >"$D/xterm-window.json"
    n=$(routes keyboard)
    d0=$(done_count)
    send_final "$TEXT
zwei"
    check "xterm: route keyboard (terminal without input method)" wait_route keyboard "$n"
    check "xterm: exact UTF-8 and Return" nested_wait 8 bytes_are "$D/xterm.bin" "$(python3 -c 'import sys; print((sys.argv[1] + "\r" + "zwei").encode().decode("latin-1").encode("unicode_escape").decode(), end="")' "$TEXT")"
    wait_done "$d0"
else
    bad "xterm skipped: no safe X display (${XDISP:-none})"
fi

# --- 20. Beenden während des Tippens lässt alles los -------------------------------------------------------------
set_config '{"typing": {"output": "type", "delay_ms": 40, "keyboard_only_app_ids": ["dictate-test-kbd"]}, "actions": {"terminal_classes": ["Alacritty", "XTerm", "xterm"]}}' || bad "config reload (slow)"
focus "$KBAPP"
b0=$(tlen kb)
send_final "$(python3 -c 'print("klmnopqrst" * 15, end="")')"
grown10() { (($(tlen kb) >= b0 + 10)); }
nested_wait 5 grown10
kill -TERM "$UI_PID"
check "sigterm: dictate-ui ends during typing" nested_wait 5 bash -c "! kill -0 $UI_PID 2>/dev/null"
UI_PID=""
l1=$(tlen kb)
sleep 1.2
l2=$(tlen kb)
check "sigterm: no key repeats after the exit ($l1 → $l2)" test "$l1" -eq "$l2"
check "sigterm: output thread ended in order" grep -q 'text output ended' "$UI_LOG"
