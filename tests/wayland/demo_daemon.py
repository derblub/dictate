#!/usr/bin/env python3
# Copyright (C) 2026 Daniel Kurdoghlian
# Pushing Pixels, Vienna <https://pushingpixels.at>
# SPDX-License-Identifier: GPL-3.0-or-later
"""Demo-Daemon für die Bilder der README (tests/wayland/readme-shots.sh): der Mock des Prüfschritts `wayland`
(`mock_daemon.py`, unverändert) mit englischen Beispielinhalten statt der Testtexte. Lädt den Quelltext des Mocks,
führt ihn ohne den Aufruf von `main()` aus, ersetzt Mitschrift, Antwort der Textaktion, Kandidaten der Palette und die
Inhalte der Diagnose und startet dann den Server des Mocks. Alle Werte sind erfunden (Hosts, Personen, Pfade).

Aufruf wie der Mock: demo_daemon.py SOCKET LOGFILE [MAX_LISTEN_S]
"""
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
with open(os.path.join(HERE, "mock_daemon.py"), encoding="utf-8") as f:
    source = f.read()
assert source.rstrip().endswith("\nmain()"), "mock_daemon.py no longer ends with main()"
m = {"__name__": "mock_daemon", "__file__": os.path.join(HERE, "mock_daemon.py")}
exec(compile(source.rstrip()[: -len("main()")], m["__file__"], "exec"), m)

MODEL, HOST = "claude-haiku-5-5", "api.anthropic.com"

# --- Overlay: Mitschrift beim Sprechen (sicherer Teil, unsicherer Rest) --------------------------------------------
m["PARTIAL"] = ("Remind me to send the quarterly numbers to Anna before lunch, and book a table for four",
                "on Friday at seven")
m["FINAL"] = "Remind me to send the quarterly numbers to Anna before lunch, and book a table for four on Friday at seven."

# --- Textaktion: Erklärung einer markierten Fehlermeldung ----------------------------------------------------------
m["ANSWER_PARTS"] = [
    "Python cannot find the package `requests`. It is not installed in the environment that runs the script, ",
    "often because the script runs with a different interpreter than the one you installed it for.\n\n",
    "**Fix:** install it for the same interpreter:\n\n",
    "```sh\npython3 -m pip install requests\n```\n\n",
    "If the project uses a virtual environment, activate it first (`source .venv/bin/activate`) ",
    "and run the command there.\n",
]
_grab = m["act_grab"]


def act_grab(req):
    reply = _grab(req)
    if reply.get("ok"):
        reply["target"] = {"host": HOST, "model": MODEL, "cloud": True}
        for a in reply.get("actions", []):
            a["model"] = MODEL
    return reply


m["act_grab"] = act_grab

# --- Befehlspalette: Kandidaten für „fi“ in der Form des Resolvers (src/palette/resolve.rs) -------------------------
_resolve = m["pal_resolve"]


def pal_resolve(req):
    if (req.get("input") or "").strip() != "fi":
        return _resolve(req)
    m["log"]("pal_resolve 2")
    wins = [w for w in (req.get("context") or {}).get("windows", []) if isinstance(w, dict) and "id" in w]
    wid = wins[0]["id"] if wins else 1

    def app(app_id, name, generic, argv):
        return {"intent": {"intent": "launch_app", "query": name},
                "target": {"kind": "app", "id": app_id, "argv": argv, "working_dir": None, "terminal": False},
                "display": f"{name} · Launch", "detail": generic, "score": 0.8, "class": "run", "target_id": app_id}

    cands = [
        {"intent": {"intent": "focus_window", "query": "Firefox"}, "target": {"kind": "window", "id": wid},
         "display": "Firefox · Switch to window", "detail": "Release notes · Workspace 2", "score": 1.0, "class": "run",
         "target_id": str(wid)},
        app("firefox.desktop", "Firefox", "Web Browser", ["firefox"]),
        app("org.gnome.Nautilus.desktop", "Files", "File Manager", ["nautilus"]),
        {"intent": {"intent": "open_folder", "query": "field-notes"},
         "target": {"kind": "project", "path": "/home/sam/code/field-notes"},
         "display": "Open folder field-notes", "detail": "/home/sam/code/field-notes", "score": 0.6, "class": "run",
         "target_id": "/home/sam/code/field-notes"},
    ]
    return {"ok": True, "candidates": cands, "confident": True, **m["exec_prefs"]()}


m["pal_resolve"] = pal_resolve

# --- Diagnose: letzter Boot mit einem Absturz im Grafiktreiber ------------------------------------------------------
m["VALUE_HOST"] = "studio-pc"
m["VALUE_USER"] = "sam"
BODIES = {
    "boot_end": [
        "Nov 04 09:47:41 host-1 systemd[1]: Started Backup of /home/user-1.",
        "Nov 04 09:47:52 host-1 kernel: NVRM: GPU at PCI:0000:01:00: GPU has fallen off the bus.",
        "Nov 04 09:47:52 host-1 kernel: NVRM: Xid (PCI:0000:01:00): 79, GPU has fallen off the bus.",
        "Nov 04 09:47:55 host-1 niri[1187]: GPU reset, recreating the renderer",
        "-- journal ends here, no shutdown entry --",
    ],
    "kernel_errors": [
        "<3>[ 7489.002114] NVRM: Xid (PCI:0000:01:00): 79, GPU has fallen off the bus.",
        "<3>[ 7489.002190] NVRM: GPU 0000:01:00.0: RmInitAdapter failed!",
    ],
    "pstore": [
        "dump 2026-11-04T09:47:57+0100 (1793780877/001), dmesg.txt",
        "<4>[ 7497.083230] Oops: general protection fault in nvidia_isr",
        "<4>[ 7497.083231] CPU: 5 PID: 0 Comm: swapper/5 Tainted: P O 7.2.9-arch1-1",
        "<0>[ 7497.083233] Kernel panic - not syncing: Fatal exception in interrupt",
    ],
}


def diag_section_body(n, probe, rnd):
    lines = BODIES.get(probe, [f"{probe}: nothing unusual"])
    return [f"{i + 1}| {line}" for i, line in enumerate(lines)]


m["diag_section_body"] = diag_section_body
_preview = m["diag_preview_json"]


def diag_preview_json():
    reply = _preview()
    reply["text"] = reply["text"].replace(m["DIAG_MARK"] + " ", "")
    return reply


m["diag_preview_json"] = diag_preview_json


def diag_answer():
    secs = m["diag_sections"]()
    sid = {probe: s for s, probe, _, _, _ in secs}
    cause = "The NVIDIA driver lost the GPU on host-1 and crashed in its interrupt handler"
    second = "A full disk or the backup is unlikely: the journal shows no errors before the GPU fault"
    notes = ("Xid 79 usually means the card stopped answering on the PCIe bus: power, heat or a loose card, "
             "less often the driver.\n\n"
             "- Check the GPU temperature log of host-1 around 09:47.\n"
             "- If it happens again, try the previous driver version.\n")
    spans_c = [{"start": cause.index("host-1"), "end": cause.index("host-1") + 6, "placeholder": "host-1", "value": m["VALUE_HOST"]}]
    i = notes.index("host-1")
    spans_n = [{"start": i, "end": i + 6, "placeholder": "host-1", "value": m["VALUE_HOST"]}]
    answer = {
        "findings": [
            {"cause": cause, "confidence": "high",
             "evidence": [{"section": sid.get("pstore", "S1"), "from": 2, "to": 4}, {"section": sid.get("boot_end", "S1"), "from": 2, "to": 3}]},
            {"cause": second, "confidence": "low", "evidence": [{"section": sid.get("boot_end", "S1"), "from": 1, "to": 1}]},
        ],
        "checks": [
            {"probe": {"probe": "kernel_errors", "boot": -2}, "label": "kernel_errors · boot -2",
             "reason": "Did the same fault happen in the boot before?", "network": False},
            {"probe": {"probe": "package_changes", "days": 14}, "label": "package_changes · 14 days",
             "reason": "Was the driver updated shortly before?", "network": False},
        ],
        "notes": notes,
    }
    return answer, {"findings": [spans_c, []], "notes": spans_n}


m["diag_answer"] = diag_answer

m["main"]()
