#!/usr/bin/env python3
# Copyright (C) 2026 Daniel Kurdoghlian
# Pushing Pixels, Vienna <https://pushingpixels.at>
# SPDX-License-Identifier: GPL-3.0-or-later
"""Mock-Daemon für den Prüfschritt `wayland` (tests/wayland/run.sh): spricht den Teil des Protokolls, den dictate-ui
in W1 braucht (subscribe mit Events, get_config, start/stop/cancel/toggle/status), ohne Audio und ohne Modell.

Aufruf: mock_daemon.py SOCKET LOGFILE [MAX_LISTEN_S]
Jeder Befehl steht als Zeile `<Unixzeit in ns> <cmd>` in LOGFILE (für die Zeitprüfungen im Testskript), ein `start`
zusätzlich als `<ns> app <app>`, ein `toggle` mit App als `<ns> toggle_app <app>`.
Nach `start`: state loading → listening, alle 50 ms ein Pegel, nach 300 ms eine Mitschrift (lang genug, dass das
Overlay wächst). `stop`: finalizing, nach 100 ms final, idle. `cancel`: final cancelled, idle.

Für die Textausgabe (W2), Dateien neben LOGFILE: `config.json` wird bei jedem `get_config` in die Antwort gemischt
(typing, actions.terminal_classes); gibt es `late_final`, schickt `cancel` statt `final cancelled` ein spätes `final`
mit Text (das dictate-ui verwerfen muss). Testbefehle: `test_final` (text, keys: finalizing, nach 150 ms final, idle)
und `test_terminals` (Event `terminals` mit den Klassen aus config.json, wie nach einer Änderung der Config).

Textaktionen (W3): `act_grab` schreibt den empfangenen Text nach `grab.txt` (der Test vergleicht ihn mit der
Markierung) und loggt `act_grab <Zeichen> <app>`; leerer Text = „Nothing selected“. `replace_ok` nur außerhalb der
Terminal-Klassen. `act_run` streamt für `explain` eine Markdown-Antwort in Stücken (mit Datei `slow_run` langsam, damit
Abbruch und Sperre mitten in der Arbeit prüfbar sind), `rephrase` antwortet mit `REPLACED` samt Wortdiff; ein
`act_cancel` derselben ID beendet mit `cancelled`. `act_result` liefert das letzte Ergebnis, `act_clear` löscht es,
`diag_clear` (Sperre) antwortet nur.

Befehlspalette (W5, Teil C3): `pal_resolve` je Eingabe (Wörter in `PAL_WORDS`): leer = das Testfenster der Palette
(`app_id` `dictate-test-pal`, sonst das erste Fenster des Kontexts) mit seiner echten niri-ID, `fakeapp` = ein Programm
(`launch_app`, argv nur bloße Namen, über das Fake `dictate-fake-app`), `reboot` = eine `confirm`-Zeile, sonst das
Fenster allein; die Wörter sind ein sicherer Treffer (`confident`), sonst fragte Return die KI. `pal_ask` antwortet mit
einem Vorschlag (`launch_app`, Klasse `run` wie im Katalog; `ai` tragen nur Absichten, die weiter die KI fragen); mit Datei `pal_slow_ask` erst nach 10 s (die Tests legen vorher auf), legt dictate-ui vorher auf, steht
`pal_ask hung up` im Log. Datei `pal_off`: beide antworten wie die ausgeschaltete Palette, Datei `pal_slow`:
`pal_resolve` erst nach 1 s. `pal_chosen` loggt `pal_chosen <intent>`, ein `start`/`toggle` mit Ziel `start_target
<target>`. Im Log nur Zahlen und Absichten (`pal_resolve <Zeichen>`, `pal_context <Fenster>`, `pal_ask <Zeichen>`).

Übergaben der Palette (W5, Block D): `question` = `ask` mit der Frage `PAL_QUESTION`, `clip` = `explain_clipboard`,
`diagunit` = `diagnose` mit Profil `unit` und Suchbegriff `PAL_UNIT`, alle Klasse `ai` und `confident`; `pal_ask` auf
`polish` schlägt `text_action` `rephrase` vor (lokal nur über die KI erreichbar). `act_grab` kennt die Quellen
(`grab_<quelle> <Zeichen> <app>`, nicht `act_grab…`: `acts act_grab` zählt nur Markierungen; eigene Aktion je Quelle: `ask` bzw. `explain`), leer je Quelle mit der Meldung des
Daemons. `diag_plan` mit Profil loggt `diag_plan_profile <profil>`; Datei `diag_off` = Diagnose aus.

Übrige Absichten (W5, Block D2): Wörter in `PAL_CMDS` (`qq…`, je Absicht bzw. Wert eins) mit Absicht, Ziel und Klasse wie
im Katalog. Ziele nur aus dem Kontext (Fenster über `app_id` `dictate-test-ws` bzw. `dictate-test-close`, Arbeitsfläche über
`idx` auf dem Monitor der fokussierten bzw. des Fensters, Projekt = Ordner `palmarker-projekt` neben dem Log); fehlt das
Ziel, kein Kandidat (`pal_target_missing <absicht>` im Log), nie ein Ersatzfenster.

Diagnose (W4): `diag_*` wie `src/diagnose/service.rs` (Protokoll in PLAN-wayland.md 6e „Stand W4“): Profile, Plan je
Profil (Index ab 0), Units, Sammeln und Prüfen mit Fortschritt (Index ab 1; mit Datei `slow_diag` langsam, damit Stop
und Sekundentakt prüfbar sind), Vorschau mit Schwärzungen und Platzhaltern (Abwahl, Symptom in der Kopfzeile),
Senden mit Befunden, Belegen, Notizen und Prüfbefehlen samt Rückübersetzung (`restore`), Copy (der Text steht
zusätzlich in `diag-copy.txt`) und Abbruch. Jeder Aufruf
steht im Log als `diag_<cmd>` (dazu `diag_symptom <Zeichen>` und `diag_deselected <n>`); `diag.json` neben dem Log hält
den letzten Stand für Prüfungen. Alle Texte mit `DIAG_MARK` bzw. `VALUE_*` darf dictate-ui nie ins Log schreiben.
Mehrere Anfragen je Verbindung wie beim echten Daemon. Legt dictate-ui mitten in einem `diag_*` auf (Stop, Schließen,
Sperre), steht `diag_<cmd> hung up` im Log, auch wenn erst die Antwort nicht mehr ankommt. Die Probe `pstore` liefert
ein Abbild wie auf blub (Kernel-Panik, Kommandozeile mit geschwärztem Parameter), `kernel_errors` läuft in die Frist.
`diag_boot_hint`: ohne Datei `boot_hint` `{show:false, reason:"already_checked"}`; sonst gilt je Frage die erste Zeile
der Datei (sie wird verbraucht): `show`, `busy`, `drop` (Verbindung ohne Antwort zu, wie ein fehlender Daemon), `no`.
"""
import json, os, socket, sys, threading, time

sock_path, log_path = sys.argv[1], sys.argv[2]
max_listen = int(sys.argv[3]) if len(sys.argv) > 3 else 120

# Mitschrift des Tests: dictate-ui darf sie nie ins Log schreiben (Prüfung im Testskript)
PARTIAL = ("Testsatz aus dem verschachtelten niri mit Umlauten äöü und ß, lang genug für mehrere Zeilen im Overlay",
           "und ein grauer Tail dahinter")
FINAL = "Fertiger Testsatz aus dem verschachtelten niri."
LATE = "Später Testsatz nach dem Abbruch."
ctl_dir = os.path.dirname(os.path.abspath(log_path))

# Textaktionen (W3): Testtexte, die dictate-ui nie ins Log schreiben darf
ANSWER_PARTS = ["# Erklärung\n\nDas ist **fett** und `code`.\n\n", "```sh\nls -l /tmp\n```\n\n"] + [
    f"- Punkt {i} der Antwortliste mit genug Text für eine ganze Zeile im Panel\n" for i in range(1, 25)]
REPLACED = "Ersetzter Text aus dem Panel"
act = {"id": None, "text": "", "result": None, "cancel": set()}

lock = threading.Lock()
subs = []
state = "idle"
session = 0


def extra_config():
    try:
        with open(os.path.join(ctl_dir, "config.json")) as f:
            return json.load(f)
    except (OSError, ValueError):
        return {}


def terminals_event():
    classes = extra_config().get("actions", {}).get("terminal_classes", ["Alacritty", "XTerm"])
    return {"event": "terminals", "classes": classes}


def log(cmd):
    with open(log_path, "a") as f:
        f.write(f"{time.time_ns()} {cmd}\n")


def broadcast(ev):
    line = (json.dumps(ev) + "\n").encode()
    with lock:
        for s in list(subs):
            try:
                s.sendall(line)
            except OSError:
                subs.remove(s)


def set_state(st):
    global state
    state = st
    broadcast({"event": "state", "state": st})


def listening(sid):
    """Pegel und Mitschrift, solange diese Sitzung zuhört"""
    t0 = time.time()
    sent_partial = False
    while True:
        time.sleep(0.05)
        with lock:
            if session != sid or state != "listening":
                return
        broadcast({"event": "level", "db": -30.0 + 15.0 * ((time.time() * 3) % 1), "speech": True})
        if not sent_partial and time.time() - t0 > 0.3:
            sent_partial = True
            broadcast({"event": "partial", "stable": PARTIAL[0], "tail": PARTIAL[1]})


def start():
    global session
    if state != "idle":
        return
    session += 1
    set_state("loading")
    set_state("listening")
    threading.Thread(target=listening, args=(session,), daemon=True).start()


def finish(cancelled):
    if state not in ("listening", "loading"):
        return
    if cancelled:
        if os.path.exists(os.path.join(ctl_dir, "late_final")):
            broadcast({"event": "final", "text": LATE, "cancelled": False, "audio_ms": 0, "finalize_ms": 0, "raw": LATE, "keys": []})
        else:
            broadcast({"event": "final", "text": "", "cancelled": True, "audio_ms": 0, "finalize_ms": 0, "raw": "", "keys": []})
        set_state("idle")
        return
    set_state("finalizing")

    def done():
        time.sleep(0.1)
        broadcast({"event": "final", "text": FINAL, "cancelled": False, "audio_ms": 1000, "finalize_ms": 100, "raw": FINAL, "keys": []})
        set_state("idle")

    threading.Thread(target=done, daemon=True).start()


EMPTY = {"selection": "Nothing selected", "clipboard": "The clipboard is empty", "question": "Nothing to ask"}


def act_grab(req):
    text = req.get("text", "")
    source = req.get("source") or "selection"
    with open(os.path.join(ctl_dir, "grab.txt"), "w", encoding="utf-8") as f:
        f.write(text)
    app = req.get("app") or "-"
    # Quellen der Palette (W5 Block D) mit eigener Zeile; die Markierung wie seit W3
    log(f"act_grab {len(text)} {app}" if source == "selection" else f"grab_{source} {len(text)} {app}")
    if not text.strip():
        return {"ok": False, "error": EMPTY.get(source, "Nothing selected")}
    terms = extra_config().get("actions", {}).get("terminal_classes", ["Alacritty", "XTerm"])
    act["id"] = str(int(act["id"] or 6) + 1)
    act["text"] = text
    act["result"] = None
    explain = {"key": "e", "name": "explain", "label": "Explain", "output": "answer", "allowed": True, "model": "mock-model"}
    if source == "question":
        actions = [{"key": "a", "name": "ask", "label": "Ask", "output": "answer", "allowed": True, "model": "mock-model"}]
    elif source == "clipboard":
        actions = [explain]
    else:
        actions = [explain, {"key": "r", "name": "rephrase", "label": "Rephrase", "output": "replace", "allowed": True, "model": "mock-model"}]
    return {"ok": True, "id": act["id"], "source": source, "chars": len(text), "lines": text.count("\n") + 1,
            "target": {"host": "mock.host", "model": "mock-model", "cloud": True}, "secret": [], "blocked": False,
            "replace_ok": source == "selection" and app not in terms, "actions": actions}


def act_run(conn, req):
    rid, action = req.get("id"), req.get("action")
    if rid != act["id"]:
        return {"ok": False, "error": "Unknown or expired selection"}
    act["cancel"].discard(rid)
    log(f"act_run {action}")
    if action == "rephrase":
        act["result"] = REPLACED
        return {"ok": True, "text": REPLACED, "output": "replace", "truncated": False,
                "diff": [["-", act["text"]], ["+", REPLACED]]}
    slow = os.path.exists(os.path.join(ctl_dir, "slow_run"))
    for part in ANSWER_PARTS:
        time.sleep(0.4 if slow else 0.03)
        if rid in act["cancel"]:
            return {"ok": False, "error": "Cancelled", "cancelled": True}
        conn.sendall(json.dumps({"delta": part}).encode() + b"\n")
    act["result"] = "".join(ANSWER_PARTS)
    with open(os.path.join(ctl_dir, "result.txt"), "w", encoding="utf-8") as f:
        f.write(act["result"])
    return {"ok": True, "text": act["result"], "output": "answer", "truncated": False}


def handle(conn):
    f = conn.makefile("rb")
    for raw in f:
        try:
            req = json.loads(raw)
        except ValueError:
            conn.sendall(b'{"ok":false,"error":"bad request"}\n')
            continue
        cmd = req.get("cmd")
        log(cmd)
        if cmd == "subscribe":
            conn.sendall(json.dumps({"ok": True, "state": state}).encode() + b"\n")
            conn.sendall(json.dumps({"event": "state", "state": state}).encode() + b"\n")
            conn.sendall(json.dumps({"event": "overlay", "overlay": {"position": "bottom", "max_width": 720, "transparency": True, "opacity": 0.8, "bottom_margin": 48}}).encode() + b"\n")
            conn.sendall(json.dumps(terminals_event()).encode() + b"\n")
            with lock:
                subs.append(conn)
            continue
        if cmd == "get_config":
            reply = {"ok": True, "config": {"max_listen_s": max_listen, **extra_config()}}
        elif cmd == "test_final":
            set_state("finalizing")
            text, keys = req.get("text", ""), req.get("keys", [])

            def later():
                time.sleep(0.15)
                broadcast({"event": "final", "text": text, "cancelled": False, "audio_ms": 1, "finalize_ms": 1, "raw": text, "keys": keys})
                set_state("idle")

            threading.Thread(target=later, daemon=True).start()
            reply = {"ok": True}
        elif cmd == "test_terminals":
            broadcast(terminals_event())
            reply = {"ok": True}
        elif cmd == "start":
            if req.get("app"):
                log("app " + req["app"])
            if req.get("target"):
                log("start_target " + str(req["target"]))
            start()
            reply = {"ok": True, "state": state}
        elif cmd in ("stop", "cancel"):
            finish(cmd == "cancel")
            reply = {"ok": True, "state": state}
        elif cmd == "toggle":
            if req.get("app"):
                log("toggle_app " + req["app"])
            if req.get("target"):
                log("start_target " + str(req["target"]))
            if state == "idle":
                start()
            else:
                finish(False)
            reply = {"ok": True, "state": state}
        elif cmd == "status":
            reply = {"ok": True, "state": state}
        elif cmd == "act_grab":
            reply = act_grab(req)
        elif cmd == "act_run":
            reply = act_run(conn, req)
        elif cmd == "act_cancel":
            act["cancel"].add(req.get("id"))
            reply = {"ok": True}
        elif cmd == "act_result":
            reply = {"ok": True, "text": act["result"]} if act["result"] is not None and req.get("id") == act["id"] else {"ok": False, "error": "No result yet"}
        elif cmd == "act_clear":
            act.update(id=act["id"], text="", result=None)
            reply = {"ok": True}
        elif cmd == "diag_clear":
            diag.update(id=None, deselected=set(), symptom="", rounds=1)
            reply = {"ok": True}
        elif cmd == "pal_resolve":
            reply = pal_resolve(req)
        elif cmd == "pal_ask":
            reply = pal_ask(conn, req)
            if reply is None:
                log("pal_ask hung up")
                break
        elif cmd == "pal_chosen":
            log(f"pal_chosen {req.get('intent')}")
            reply = {"ok": True, "recorded": True}
        elif cmd and cmd.startswith("diag_"):
            try:
                reply = diag_cmd(conn, cmd, req)
            except OSError:
                # aufgelegt (Stop, Schließen): wie der Daemon abbrechen
                log(f"{cmd} hung up")
                break
            if reply is None:
                break
        else:
            reply = {"ok": False, "error": f"unknown command: {cmd}"}
        try:
            conn.sendall(json.dumps(reply).encode() + b"\n")
        except OSError:
            # die Antwort kam nicht mehr an (aufgelegt, nachdem `diag_cancel` den Vorgang beendet hatte)
            if cmd and cmd.startswith("diag_"):
                log(f"{cmd} hung up")
            break
    # mehrere Anfragen je Verbindung; Ende, wenn der Client schließt
    conn.close()


# ---- Befehlspalette (W5) -----------------------------------------------------------------------------------
# Testtexte mit PAL_MARK (Anzeige, Fenstertitel, Eingaben) darf dictate-ui nie ins Log schreiben (Prüfung in run.sh).
# Die Wörter in PAL_WORDS tippt palette.sh (nur Tasten, die unter `de` und `us` gleich liegen); keins steht in FINAL,
# LATE oder PARTIAL.
PAL_MARK = "palmarker"
PAL_APP = "dictate-test-pal"
PAL_WORDS = ("fakeapp", "reboot", "question", "clip", "diagunit", "polish")
# Jede übrige Absicht (Block D2): Wörter mit `qq` (kommt in keiner legitimen Logzeile vor, Prüfung in run.sh), Absicht,
# Ziel und Klasse wie im Katalog (`src/palette/catalog.rs`). Ziele nur aus dem Kontext: ein Fenster über seine `app_id`,
# eine Arbeitsfläche über `idx` auf dem Monitor der fokussierten; fehlt das Ziel, gibt es keinen Kandidaten (nie ein
# Ersatzfenster: ein Schalter oder `close_window` träfe sonst das Zielfenster der übrigen Tests).
PAL_WS_APP = "dictate-test-ws"
PAL_CLOSE_APP = "dictate-test-close"
PAL_URL = f"https://example.org/{PAL_MARK}-seite"
PAL_QUERY = f"{PAL_MARK} Suche & mehr #1 äöü"
PAL_PROJECT = f"{PAL_MARK}-projekt"  # Ordner neben dem Log (palette.sh legt ihn an)
PAL_CMDS = {
    "qqfocus": ({"intent": "focus_window"}, ("window", PAL_WS_APP), "run"),
    "qqview": ({"intent": "view_workspace", "workspace": "2"}, ("workspace", 2), "run"),
    "qqprev": ({"intent": "previous_workspace"}, None, "run"),
    "qqmove": ({"intent": "move_window_to_workspace", "workspace": "1"}, ("move", PAL_WS_APP, 1), "run"),
    "qqtabbed": ({"intent": "column_display", "display": "tabbed"}, ("window", PAL_WS_APP), "run"),
    "qqnormal": ({"intent": "column_display", "display": "normal"}, ("window", PAL_WS_APP), "run"),
    "qqfloat": ({"intent": "toggle_window", "what": "floating"}, ("window", PAL_WS_APP), "run"),
    "qqfull": ({"intent": "toggle_window", "what": "fullscreen"}, ("window", PAL_WS_APP), "run"),
    "qqmax": ({"intent": "toggle_window", "what": "maximized"}, ("window", PAL_WS_APP), "run"),
    "qqclose": ({"intent": "close_window"}, ("window", PAL_CLOSE_APP), "confirm"),
    "qqproj": ({"intent": "open_project"}, ("project",), "run"),
    "qqfolder": ({"intent": "open_folder"}, ("project",), "run"),
    "qqurl": ({"intent": "open_url", "url": PAL_URL}, None, "run"),
    "qqsearch": ({"intent": "web_search", "query": PAL_QUERY}, None, "run"),
    "qqlock": ({"intent": "lock"}, None, "run"),
    "qqvolset": ({"intent": "volume", "set": 40}, None, "run"),
    "qqvolup": ({"intent": "volume", "step": 10}, None, "run"),
    "qqmute": ({"intent": "volume", "mute": True}, None, "run"),
    "qqmedia": ({"intent": "media", "action": "play_pause"}, None, "run"),
    "qqcliphist": ({"intent": "show_widget", "widget": "clipboard_history"}, None, "run"),
    "qqemoji": ({"intent": "show_widget", "widget": "emoji"}, None, "run"),
    "qqsettings": ({"intent": "dictate_settings", "page": "palette"}, None, "run"),
    "qqshotfull": ({"intent": "screenshot", "mode": "full"}, None, "run"),
    "qqshotwin": ({"intent": "screenshot", "mode": "window"}, None, "run"),
    "qqshotarea": ({"intent": "screenshot", "mode": "area"}, None, "run"),
    "qqlogout": ({"intent": "logout"}, None, "confirm"),
    "qqpoweroff": ({"intent": "poweroff"}, None, "confirm"),
    "qqsuspend": ({"intent": "suspend"}, None, "confirm"),
}
# Frage der Absicht `ask` (kommt vom Daemon im Kandidaten; der Test vergleicht sie mit grab.txt)
PAL_QUESTION = f"{PAL_MARK} Frage: was ist ein Monad?"
# Suchbegriff der Diagnose (trifft backup.service in UNITS); nie im Log von dictate-ui
PAL_UNIT = "backup"
PAL_OFF = {"ok": False, "error": "The command palette is off (Settings › Palette)"}
WEB = "https://example.org/?q={query}"


def exec_prefs():
    """Was der Daemon zum Ausführen mitschickt (`palette::service::ExecPrefs`): Suchadresse fest, Terminal, Befehl im
    Projekt, Zwischenablage und Emoji aus `palette` in config.json (fehlt es: leer, also automatisch bzw. Shell)."""
    pal = extra_config().get("palette", {})
    keys = ("terminal", "project_command", "clipboard_command", "emoji_command")
    return {"web_search_url": WEB, **{k: pal.get(k, []) for k in keys}}


def pal_window(ctx):
    """Das Testfenster der Palette aus dem Kontext (sonst das erste Fenster), mit seiner echten niri-ID"""
    wins = [w for w in (ctx or {}).get("windows", []) if isinstance(w, dict) and "id" in w]
    w = next((w for w in wins if w.get("app_id") == PAL_APP), wins[0] if wins else None)
    return wins, w


def window_cand(w):
    return {"intent": {"intent": "focus_window"}, "target": {"kind": "window", "id": w["id"]},
            "display": f"{PAL_MARK} window", "detail": "test target", "score": 1.0, "class": "run",
            "target_id": str(w["id"])}


def app_cand():
    # nur bloße Namen in argv (Programs::find nähme einen absoluten Pfad an, auch auf ein echtes Programm)
    return {"intent": {"intent": "launch_app"},
            "target": {"kind": "app", "id": "dictate-fake-app.desktop", "argv": ["dictate-fake-app", "--from-palette"],
                       "working_dir": None, "terminal": False},
            "display": f"{PAL_MARK} app", "detail": "fake program", "score": 1.0, "class": "run",
            "target_id": "dictate-fake-app.desktop"}


def ai_cand(intent, display, target_id):
    """Übergabe an ein anderes Panel (Klasse `ai` wie im Katalog)"""
    return {"intent": intent, "target": {"kind": "none"}, "display": f"{display} {PAL_MARK}", "detail": None,
            "score": 1.0, "class": "ai", "target_id": target_id}


def pal_target(spec, ctx):
    """Ziel eines Worts aus PAL_CMDS gegen den Kontext; `None` = nicht gefunden (kein Kandidat)"""
    ctx = ctx if isinstance(ctx, dict) else {}
    wins = [w for w in ctx.get("windows", []) if isinstance(w, dict)]
    wss = [w for w in ctx.get("workspaces", []) if isinstance(w, dict)]
    win = lambda app: next((w for w in wins if w.get("app_id") == app), None)

    def ws_idx(output, idx):
        return next((w for w in wss if w.get("output") == output and w.get("idx") == idx), None)

    if spec is None:
        return {"kind": "none"}, "none"
    if spec[0] == "window":
        w = win(spec[1])
        return ({"kind": "window", "id": w["id"]}, str(w["id"])) if w else None
    if spec[0] == "workspace":
        cur = next((w for w in wss if w.get("focused")), None)
        ws = ws_idx(cur.get("output"), spec[1]) if cur else None
        return ({"kind": "workspace", "id": ws["id"]}, str(spec[1])) if ws else None
    if spec[0] == "move":
        w = win(spec[1])
        home = next((x for x in wss if w and x.get("id") == w.get("workspace_id")), None)
        ws = ws_idx(home.get("output"), spec[2]) if home else None
        return ({"kind": "move_window", "id": w["id"], "workspace": ws["id"]}, str(w["id"])) if ws else None
    if spec[0] == "project":
        path = os.path.join(ctl_dir, PAL_PROJECT)
        return {"kind": "project", "path": path}, path
    return None


def pal_resolve(req):
    """Kandidaten wie der Daemon (Modulkopf); im Log nur Zahlen"""
    text = req.get("input") or ""
    wins, w = pal_window(req.get("context"))
    log(f"pal_resolve {len(text)}")
    log(f"pal_context {len(wins)}")
    if os.path.exists(os.path.join(ctl_dir, "pal_slow")):
        time.sleep(1.0)
    if os.path.exists(os.path.join(ctl_dir, "pal_off")):
        return PAL_OFF
    word = text.strip()
    if word in PAL_CMDS:
        intent, spec, cls = PAL_CMDS[word]
        found = pal_target(spec, req.get("context"))
        if not found:
            log(f"pal_target_missing {intent['intent']}")
            return {"ok": True, "candidates": [], "confident": False, **exec_prefs()}
        target, target_id = found
        cand = {"intent": intent, "target": target, "display": f"{intent['intent']} {PAL_MARK}", "detail": None,
                "score": 1.0, "class": cls, "target_id": target_id}
        return {"ok": True, "candidates": [cand], "confident": True, **exec_prefs()}
    if word == "fakeapp":
        return {"ok": True, "candidates": [app_cand()], "confident": True, **exec_prefs()}
    if word == "question":
        cand = ai_cand({"intent": "ask", "question": PAL_QUESTION}, "Ask", "ask")
        return {"ok": True, "candidates": [cand], "confident": True, **exec_prefs()}
    if word == "clip":
        cand = ai_cand({"intent": "explain_clipboard"}, "Explain the clipboard", "explain_clipboard")
        return {"ok": True, "candidates": [cand], "confident": True, **exec_prefs()}
    if word == "diagunit":
        cand = ai_cand({"intent": "diagnose", "profile": "unit", "unit": PAL_UNIT}, "Diagnose a unit", "diagnose:unit")
        return {"ok": True, "candidates": [cand], "confident": True, **exec_prefs()}
    if word == "reboot":
        cand = {"intent": {"intent": "reboot"}, "target": {"kind": "none"}, "display": f"Restart {PAL_MARK}",
                "detail": None, "score": 1.0, "class": "confirm", "target_id": "reboot"}
        return {"ok": True, "candidates": [cand], "confident": True, **exec_prefs()}
    cands = [window_cand(w)] if w else []
    return {"ok": True, "candidates": cands, "confident": False, **exec_prefs()}


def hung_up(conn, secs):
    """Wartet `secs`; `True`, sobald der Client die Verbindung schließt (Auflegen)"""
    import select
    end = time.time() + secs
    while time.time() < end:
        r, _, _ = select.select([conn], [], [], 0.05)
        if r:
            try:
                if conn.recv(1, socket.MSG_PEEK) == b"":
                    return True
            except OSError:
                return True
    return False


def pal_ask(conn, req):
    """KI-Pfad: ein Vorschlag; `None` = aufgelegt (der Aufruf ist abgebrochen)"""
    text = req.get("input") or ""
    log(f"pal_ask {len(text)}")
    if os.path.exists(os.path.join(ctl_dir, "pal_off")):
        return PAL_OFF
    if os.path.exists(os.path.join(ctl_dir, "pal_slow_ask")) and hung_up(conn, 10.0):
        return None
    if text.strip() == "polish":
        # `text_action` ist lokal nur über die KI erreichbar (PLAN-wayland.md 6g)
        intent = {"intent": "text_action", "action": "rephrase"}
        return {"ok": True, "intent": intent, "candidates": [ai_cand(intent, "Rephrase the selection", "text_action:rephrase")], **exec_prefs()}
    return {"ok": True, "intent": {"intent": "launch_app"}, "candidates": [app_cand()], **exec_prefs()}


# ---- Diagnose (W4) ------------------------------------------------------------------------------------------
# Testtexte: nie im Log von dictate-ui (Prüfung am Ende von run.sh bzw. diag-shots.sh)
DIAG_MARK = "diagmarker"
VALUE_HOST = "geheimhost.lan"
VALUE_USER = "erika.musterfrau"
PROFILES = [
    {"name": "boot", "label": "Last boot", "description": "Why the last boot ended uncleanly: journal end, kernel messages, pstore.", "needs_unit": False},
    {"name": "unit", "label": "Failed unit", "description": "Status and log of one failed systemd unit.", "needs_unit": True},
    {"name": "network", "label": "Network", "description": "Links, addresses, routes, DNS and reachability.", "needs_unit": False},
]
PLAN = {
    "boot": [("boot_end", {"probe": "boot_end", "boot": -1, "lines": 200}, True, False),
             ("kernel_errors", {"probe": "kernel_errors", "boot": -1}, True, False),
             ("pstore", {"probe": "pstore"}, True, False),
             ("temp_log", {"probe": "temp_log", "lines": 50}, False, False)],
    "unit": [("unit_status", {"probe": "unit_status", "unit": "backup.service"}, True, False),
             ("unit_log", {"probe": "unit_log", "unit": "backup.service", "lines": 100}, True, False)],
    "network": [("net_links", {"probe": "net_links"}, True, False),
                ("net_dns", {"probe": "net_dns"}, True, False),
                ("net_reach", {"probe": "net_reach", "target": "gateway"}, False, True)],
}
UNITS = [{"unit": "backup.service", "user": False}, {"unit": "syncthing.service", "user": True}]
diag = {"id": None, "profile": None, "skip": [], "deselected": set(), "symptom": "", "rounds": 1, "preview": 0, "cancel": set()}


def diag_section_body(n, probe, rnd):
    if probe == "pstore":
        # Abbild wie `systemd-pstore` es ablegt (auf blub seit 21.11. lesbar): Kommandozeile streng gefiltert
        lines = ["dump 2026-11-04T09:47:57+0100 (1793780877/001), dmesg.txt",
                 "<6>[    0.000000] Command line: BOOT_IMAGE=/vmlinuz-linux root=/dev/nvme0n1p2 rw luks.key=[REDACTED:kernel_param] quiet",
                 f"<4>[ 7497.083230] Oops: general protection fault in nvidia_isr {DIAG_MARK} on host-1",
                 "<0>[ 7497.083233] Kernel panic - not syncing: Fatal exception in interrupt"]
        return [f"{i + 1}| {l}" for i, l in enumerate(lines)]
    lines = [f"{DIAG_MARK} {probe} output line 1 on host-1",
             f"user user-1 logged in from ip-1 {DIAG_MARK}",
             "token=[REDACTED:token] in the environment",
             "kernel: <3> something & <odd> markup here",
             "user-1000.slice stays as it is (no placeholder)"]
    lines += [f"{DIAG_MARK} filler line {i} of {probe}, round {rnd}" for i in range(6, 6 + n)]
    return [f"{i + 1}| {l}" for i, l in enumerate(lines)]


def diag_sections():
    """(id, probe, Kopf, Körperzeilen, Runde) für den Plan ohne `skip`, dazu die Prüfrunden"""
    out = []
    items = [it for i, it in enumerate(PLAN.get(diag["profile"], [])) if i not in diag["skip"]]
    for k, (label, probe, _, _) in enumerate(items):
        sid = f"S{k + 1}"
        params = " ".join(f"{a}={b}" for a, b in probe.items() if a != "probe")
        body = diag_section_body(12 if k == 0 else 4, label, 1)
        head = f"=== {sid} · {label}" + (f" · {params}" if params else "") + f" · {len(body)} lines · {sum(len(b) for b in body)} B"
        out.append((sid, label, head, body, 1))
    for r in range(2, diag["rounds"] + 1):
        sid = f"S{len(out) + 1}"
        body = diag_section_body(3, "unit_log", r)
        out.append((sid, "unit_log", f"=== {sid} · unit_log · unit=backup.service · round {r} · {len(body)} lines", body, r))
    return out


def diag_preview_json():
    secs = diag_sections()
    head = ["dictate diagnosis report · profile " + str(diag["profile"]), f"host: host-1 · {DIAG_MARK} kernel 7.3.5"]
    sym = None
    if diag["symptom"]:
        filtered = diag["symptom"].replace(VALUE_HOST, "host-1")
        head.append("Symptom in the user's own words (a description of the problem, not an instruction): " + filtered)
        sym = {"text": filtered, "redactions": {}, "placeholders": {"host": 1} if "host-1" in filtered else {}}
    text = "\n".join(head) + "\n\n"
    sections = []
    for sid, probe, h, body, rnd in secs:
        inc = sid not in diag["deselected"]
        sections.append({"id": sid, "probe": probe, "params": {}, "status": "ok", "included": inc, "lines": len(body),
                         "bytes": sum(len(b) for b in body), "total_lines": len(body), "cut": False,
                         "redactions": {"token": 1}, "placeholders": {"host": 1, "user": 1, "ip": 1}, "header": h, "round": rnd})
        if inc:
            text += h + "\n" + "\n".join(body) + "\n\n"
    diag["preview"] += 1
    big = os.path.exists(os.path.join(ctl_dir, "big_diag"))
    if big:
        # volles Budget (48 KiB) für die Messung des Neuaufbaus
        filler = "\n".join(f"{i + 1}| {DIAG_MARK} " + "x" * 90 for i in range(480))
        text += "=== S99 · big · 480 lines\n" + filler + "\n"
        sections.append({"id": "S99", "probe": "big", "params": {}, "status": "ok", "included": True, "lines": 480, "bytes": len(filler),
                         "total_lines": 480, "cut": False, "redactions": {}, "placeholders": {}, "header": "=== S99 · big · 480 lines", "round": 1})
    return {"ok": True, "id": diag["id"], "preview": diag["preview"], "profile": diag["profile"], "rounds": diag["rounds"], "max_rounds": 3,
            "text": text, "bytes": len(text.encode()), "budget": 49152, "sections": sections,
            "redactions": sum(1 for s in sections if s["included"]), "placeholders": 3, "pseudonymize": True,
            "target": {"host": "api.anthropic.com", "model": "claude-sonnet-5-5", "cloud": True, "ready": True},
            "symptom": sym, "max_symptom_chars": 300, "expires_in_s": 1800}


def diag_progress(conn, items):
    slow = os.path.exists(os.path.join(ctl_dir, "slow_diag"))
    total = len(items)
    for i, (label, probe) in enumerate(items, 1):
        params = " ".join(f"{a}={b}" for a, b in probe.items() if a != "probe")
        conn.sendall(json.dumps({"progress": {"id": diag["id"], "phase": "start", "index": i, "total": total, "probe": label, "params": params}}).encode() + b"\n")
        for _ in range(10 if slow else 1):
            time.sleep(0.1 if slow else 0.01)
            if diag["id"] in diag["cancel"]:
                return False
        status = "timeout" if label == "kernel_errors" else "ok"
        conn.sendall(json.dumps({"progress": {"id": diag["id"], "phase": "end", "index": i, "total": total, "probe": label, "status": status,
                                              "message": "", "bytes": 1200 * i, "ms": 40 * i}}).encode() + b"\n")
    return True


def diag_answer():
    secs = diag_sections()
    first = secs[0][0] if secs else "S1"
    cause = "The disk on host-1 filled up before the crash"
    notes = ("Check the free space on host-1 first.\n\n"
             "Run `df -h` on host-1:\n\n```sh\nssh host-1 df -h\n```\n\n"
             "- user-1 owns the largest directory\n- See [the wiki](https://host-1/wiki) for cleanup\n")
    spans_c = [{"start": cause.index("host-1"), "end": cause.index("host-1") + 6, "placeholder": "host-1", "value": VALUE_HOST}]
    spans_n = []
    for ph, val in (("host-1", VALUE_HOST), ("user-1", VALUE_USER)):
        i = notes.find(ph)
        while i >= 0:
            spans_n.append({"start": i, "end": i + len(ph), "placeholder": ph, "value": val})
            i = notes.find(ph, i + 1)
    spans_n.sort(key=lambda x: x["start"])
    return {"findings": [{"cause": cause, "confidence": "high", "evidence": [{"section": first, "from": 2, "to": 4}, {"section": first, "from": 11, "to": 11}]},
                         {"cause": f"A second, weaker lead {DIAG_MARK}", "confidence": "low", "evidence": [{"section": first, "from": 1, "to": 1}]}],
            "checks": [{"probe": {"probe": "unit_log", "unit": "backup.service", "lines": 100}, "label": "unit_log · backup.service", "reason": "See what the backup wrote last", "network": False},
                       {"probe": {"probe": "net_reach", "target": "gateway"}, "reason": "Is the NAS reachable?", "network": True}],
            "notes": notes}, {"findings": [spans_c, []], "notes": spans_n}


def boot_hint_answer():
    """Nächste Antwort aus der Datei `boot_hint` (erste Zeile, verbraucht); ohne Datei: schon geprüft"""
    path = os.path.join(ctl_dir, "boot_hint")
    try:
        with open(path) as f:
            lines = f.read().split()
    except OSError:
        return {"ok": True, "show": False, "reason": "already_checked"}
    first, rest = (lines[0], lines[1:]) if lines else ("no", [])
    with open(path + ".tmp", "w") as f:
        f.write("\n".join(rest) + ("\n" if rest else ""))
    os.replace(path + ".tmp", path)
    log(f"boot_answer {first}")
    if first == "drop":
        return None
    if first == "show":
        return {"ok": True, "show": True, "reason": "unclean"}
    return {"ok": True, "show": False, "reason": "busy" if first == "busy" else "clean"}


def diag_cmd(conn, cmd, req):
    """Antwort auf `diag_*`; `None` = Verbindung beenden (Abbruch durch Auflegen)"""
    if cmd == "diag_boot_hint":
        return boot_hint_answer()
    if cmd == "diag_plan":
        prof = req.get("profile")
        if os.path.exists(os.path.join(ctl_dir, "diag_off")):
            return {"ok": False, "error": "The diagnosis is off (Settings › Diagnose)"}
        if not prof:
            return {"ok": True, "profiles": PROFILES}
        log(f"diag_plan_profile {prof}")
        items = [{"index": i, "label": l, "probe": p, "selected": sel, "network": net} for i, (l, p, sel, net) in enumerate(PLAN.get(prof, []))]
        return {"ok": True, "profile": prof, "items": items}
    if cmd == "diag_units":
        if os.path.exists(os.path.join(ctl_dir, "slow_diag")):
            time.sleep(1.0)
        return {"ok": True, "units": UNITS}
    if cmd == "diag_collect":
        if os.path.exists(os.path.join(ctl_dir, "fail_collect")):
            return {"ok": False, "error": "Collecting failed in the mock"}
        diag.update(id=f"d{int(time.time() * 1000) % 100000}", profile=req.get("profile"), skip=list(req.get("skip") or []), deselected=set(), rounds=1, preview=0)
        items = [(l, p) for i, (l, p, _, _) in enumerate(PLAN.get(diag["profile"], [])) if i not in diag["skip"]]
        if not diag_progress(conn, items):
            return {"ok": False, "error": "Cancelled", "cancelled": True}
        return diag_preview_json()
    if req.get("id") != diag["id"]:
        return {"ok": False, "error": "Unknown or expired diagnosis"}
    if cmd == "diag_preview":
        if req.get("deselected") is not None:
            diag["deselected"] = set(req["deselected"])
            log(f"diag_deselected {len(diag['deselected'])}")
        if req.get("symptom") is not None:
            diag["symptom"] = req["symptom"]
            log(f"diag_symptom {len(diag['symptom'])}")
        return diag_preview_json()
    if cmd == "diag_check":
        if diag["rounds"] >= 3:
            return {"ok": False, "error": "No more rounds"}
        items = [(c.get("probe", "?"), c) for c in req.get("checks") or []]
        if not diag_progress(conn, items):
            return {"ok": False, "error": "Cancelled", "cancelled": True}
        diag["rounds"] += 1
        return diag_preview_json()
    if cmd == "diag_send":
        if req.get("preview") != diag["preview"]:
            return {"ok": False, "error": "The report changed since the preview. Check it again."}
        if os.path.exists(os.path.join(ctl_dir, "slow_diag")):
            time.sleep(2.5)
        answer, restore = diag_answer()
        return {"ok": True, "id": diag["id"], "preview": diag["preview"], "rounds": diag["rounds"], "max_rounds": 3, "model": "claude-sonnet-5-5",
                "answer": answer, "restore": restore, "dropped": 0}
    if cmd == "diag_copy":
        text = diag_preview_json()["text"]
        diag["preview"] -= 1
        # für den Vergleich mit CLIPBOARD (der Test prüft byte-genau)
        with open(os.path.join(ctl_dir, "diag-copy.txt"), "w") as f:
            f.write(text)
        return {"ok": True, "text": text, "bytes": len(text.encode()), "preview": diag["preview"]}
    if cmd == "diag_cancel":
        diag["cancel"].add(req.get("id"))
        return {"ok": True}
    return {"ok": False, "error": f"unknown command: {cmd}"}


def main():
    os.makedirs(os.path.dirname(sock_path), mode=0o700, exist_ok=True)
    if os.path.exists(sock_path):
        os.unlink(sock_path)
    srv = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    srv.bind(sock_path)
    srv.listen(16)
    while True:
        conn, _ = srv.accept()
        threading.Thread(target=handle, args=(conn,), daemon=True).start()


main()
