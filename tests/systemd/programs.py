#!/usr/bin/env python3
# Copyright (C) 2026 Daniel Kurdoghlian
# Pushing Pixels, Vienna <https://pushingpixels.at>
# SPDX-License-Identifier: GPL-3.0-or-later
"""Vergleicht die Programme, die dictate startet, ohne und unter der Härtung ihrer Unit.

    tests/systemd/programs.py [--drop KEY ...] [--diff N]

Die Kindprozesse erben die Härtung: unter `dictate.service` die Programme der Linux-Diagnose (eine argv je Programm
des Katalogs `src/diagnose/argv.rs`, dieselben festen Vorlagen), unter `dictate-ui.service` die Kurzläufer der
Befehlspalette (`src/bin/dictate-ui/pal_host.rs`), hier nur in lesenden Aufrufen. Jeder Befehl läuft einmal als
flüchtige Unit ohne Härtung und einmal mit; verglichen werden Rückgabewert, Fehlerausgabe und Ausgabe. Eine Zeile je
Befehl: `same` (gleich), `varies` (gleich bis auf Ziffern und Zeilenfolge: Zähler, Zeiten, Messwerte), `DIFFERENT`
(anderer Rückgabewert oder anderer Text, etwa eine fehlende Prozesszuordnung). Weicht der Text ab, läuft der Befehl
ein drittes Mal ohne Härtung: gleicht dieser Lauf dem unter der Unit, ist es `varies`; unterscheiden sich schon die
beiden Läufe ohne Härtung, ist es `unstable` (das System hat sich zwischendurch geändert; noch einmal laufen lassen). Mit `--diff N` folgen bis zu N abweichende Zeilen je
Befehl (Ausgaben des eigenen Rechners: nicht in Berichte kopieren). Endet mit 1, wenn etwas `DIFFERENT` ist.

Liest nur; ein Ping geht an 127.0.0.1. Kein Befehl öffnet ein Fenster; nur `noctalia msg bluetooth-status` bekommt
`WAYLAND_DISPLAY` der Sitzung, weil Noctalia darüber den Socket seiner laufenden Shell findet (eine lesende Abfrage).
Programme, die es nicht gibt, werden übersprungen.
"""

import argparse
import difflib
import itertools
import os
import pathlib
import re
import subprocess
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import unit as u  # noqa: E402

DAEMON = [
    ["journalctl", "--list-boots", "--no-pager", "-o", "json"],
    ["journalctl", "-k", "--boot=0", "-p", "warning", "-o", "short-iso", "--no-pager", "-n", "50"],
    ["journalctl", "-u", "systemd-journald.service", "--boot=0", "-n", "50", "-o", "short-iso", "--no-pager"],
    ["systemctl", "list-units", "--all", "--no-pager", "-o", "json"],
    ["systemctl", "--user", "list-units", "--all", "--no-pager", "-o", "json"],
    ["systemctl", "status", "--no-pager", "-n", "0", "--", "systemd-journald.service"],
    ["systemctl", "list-timers", "--all", "--no-pager", "-o", "json"],
    ["coredumpctl", "list", "--since=-6h", "--no-pager", "-q", "--json=short"],
    ["docker", "ps", "-a", "--format", "json"],
    ["docker", "system", "df"],
    ["ip", "-j", "addr"],
    ["ip", "-j", "route"],
    ["resolvectl", "status", "--no-pager"],
    ["ss", "-Htulpn"],
    ["tailscale", "status", "--json"],
    ["findmnt", "-J", "-t", "cifs,nfs,nfs4,fuse.sshfs"],
    ["ping", "-c", "3", "-W", "1", "--", "127.0.0.1"],
    ["df", "-h", "-l", "-x", "tmpfs", "-x", "devtmpfs", "-x", "overlay"],
    ["lsblk", "-J", "-o", "NAME,SIZE,TYPE,MOUNTPOINTS,FSTYPE"],
    ["sensors", "-j"],
    ["nvidia-smi", "--query-gpu=name,driver_version,temperature.gpu,memory.used,memory.total", "--format=csv"],
    ["pacman", "-Q", "linux", "systemd"],
]
UI = [
    ["pactl", "get-sink-volume", "@DEFAULT_SINK@"],
    ["pactl", "get-sink-mute", "@DEFAULT_SINK@"],
    ["playerctl", "--list-all"],
    ["systemctl", "--user", "is-system-running"],
    ["systemctl", "is-system-running"],
    ["loginctl", "show-session", "auto", "-p", "LockedHint"],
    # dieselbe IPC wie `noctalia msg panel-toggle clipboard`, aber nur lesend
    ["noctalia", "msg", "bluetooth-status"],
    # dieselbe polkit-Entscheidung wie `systemctl reboot|poweroff|suspend`, ohne etwas auszulösen
    ["busctl", "call", "org.freedesktop.login1", "/org/freedesktop/login1", "org.freedesktop.login1.Manager", "CanReboot"],
    ["busctl", "call", "org.freedesktop.login1", "/org/freedesktop/login1", "org.freedesktop.login1.Manager", "CanPowerOff"],
    ["busctl", "call", "org.freedesktop.login1", "/org/freedesktop/login1", "org.freedesktop.login1.Manager", "CanSuspend"],
]
def shape(text):
    """Ausgabe ohne das, was sich zwischen zwei Aufrufen ohnehin ändert: Ziffern (Zähler, Zeiten, Temperaturen) sind
    maskiert, die Zeilen sortiert (`df` und `docker` ordnen nicht fest). Fehlt unter der Unit ein Wort, etwa der
    Prozess hinter einem Port bei `ss`, bleibt das ein Unterschied."""
    return sorted(re.sub(r"[0-9]+(\.[0-9]+)?", "0", line) for line in text.splitlines())


def session_display():
    """`WAYLAND_DISPLAY` des User-Managers: `noctalia msg` findet darüber den Socket der laufenden Shell."""
    out = u.systemctl("show-environment", capture_output=True, text=True).stdout
    return next((line.split("=", 1)[1] for line in out.splitlines() if line.startswith("WAYLAND_DISPLAY=")), None)


COUNTER = itertools.count()


def run(props, argv):
    env = {"PATH": "/usr/bin", "HOME": str(pathlib.Path.home()), "XDG_RUNTIME_DIR": u.runtime_dir(), "DBUS_SESSION_BUS_ADDRESS": f"unix:path={u.runtime_dir()}/bus", "LC_ALL": "C.UTF-8"}
    if argv[0].endswith("/noctalia") and session_display():
        env["WAYLAND_DISPLAY"] = session_display()
    name = f"dictate-programs-{os.getpid()}-{next(COUNTER)}"
    child = u.transient(name, props, argv, env, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, errors="replace")
    try:
        out, err = child.communicate(timeout=60)
    except subprocess.TimeoutExpired:
        u.systemctl("stop", f"{name}.service", capture_output=True)
        out, err = child.communicate()
        return 124, out, err
    return child.returncode, out, err


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--drop", action="append", default=[], help="leave this [Service] option out")
    ap.add_argument("--add", action="append", default=[], metavar="KEY=VALUE", help="one more option, after the unit's own")
    ap.add_argument("--diff", type=int, default=0, help="show up to N differing lines per command")
    a = ap.parse_args()
    different = unstable = 0
    for unit_name, commands in (("dictate", DAEMON), ("dictate-ui", UI)):
        props = u.service_options(unit_name, drop=a.drop) + a.add
        print(f"# {unit_name}.service")
        for argv in commands:
            path = f"/usr/bin/{argv[0]}"
            label = " ".join(argv)[:70]
            if not os.access(path, os.X_OK):
                print(f"skipped   {label} (not installed)")
                continue
            rc0, out0, err0 = run([], [path, *argv[1:]])
            rc1, out1, err1 = run(props, [path, *argv[1:]])
            l0, l1 = out0.splitlines(), out1.splitlines()
            if (rc0, out0, err0) == (rc1, out1, err1):
                verdict = "same"
            elif (rc0, shape(err0), shape(out0)) == (rc1, shape(err1), shape(out1)):
                verdict = "varies"
            else:
                # ändert sich der Text auch zwischen zwei Läufen ohne Härtung (ein Container wechselt den Zustand)?
                rc2, out2, err2 = run([], [path, *argv[1:]])
                if (rc2, shape(err2), shape(out2)) == (rc1, shape(err1), shape(out1)):
                    verdict = "varies"
                elif (rc2, shape(err2), shape(out2)) != (rc0, shape(err0), shape(out0)):
                    verdict = "unstable"
                    unstable += 1
                else:
                    verdict = "DIFFERENT"
                    different += 1
            print(f"{verdict:9} {label} (exit {rc0}/{rc1}, {len(l0)}/{len(l1)} lines, stderr {len(err0)}/{len(err1)} bytes)")
            if a.diff and verdict != "same":
                lines = [d for d in difflib.unified_diff(l0 + ["[stderr]"] + err0.splitlines(), l1 + ["[stderr]"] + err1.splitlines(), lineterm="", n=0) if d[:1] in "+-" and d[:3] not in ("+++", "---")]
                for d in lines[: a.diff]:
                    print(f"          {d[:200]}")
    if unstable:
        print(f"{unstable} unstable: run again")
    sys.exit(1 if different or unstable else 0)


if __name__ == "__main__":
    main()
