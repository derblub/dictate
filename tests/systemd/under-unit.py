#!/usr/bin/env python3
# Copyright (C) 2026 Daniel Kurdoghlian
# Pushing Pixels, Vienna <https://pushingpixels.at>
# SPDX-License-Identifier: GPL-3.0-or-later
"""Startet einen Befehl unter der Härtung einer Unit aus systemd/ (flüchtige Unit im User-Manager).

    tests/systemd/under-unit.py UNIT [--rw PATH ...] [--drop KEY ...] [--show] -- CMD ARG...

UNIT ist `dictate` oder `dictate-ui`. Der Befehl läuft mit genau der Umgebung des Aufrufers (nicht der des
User-Managers) und mit dessen stdin/stdout/stderr. Die Verzeichnis-Direktiven der Unit gelten hier nicht (sie würden
die Ordner des laufenden Dienstes anfassen, tests/systemd/unit.py); was der Befehl schreiben muss, nennt `--rw`
(`ReadWritePaths=`, der Pfad muss bestehen). SIGTERM und SIGINT gehen an den Hauptprozess der Unit weiter; der Rückgabewert ist der des
Befehls. `--show` lässt systemd-run am Ende Laufzeit, CPU-Zeit und Speicherspitze der Unit auf stderr nennen;
`--plain` startet ohne jede Härtung (Vergleichslauf).

Genutzt von tests/wayland/run.sh (`DICTATE_UI_UNDER_UNIT=1`) und für die GPU-/Pulse-Tests (docs/systemd-hardening.md).
"""

import argparse
import os
import signal
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import unit as u  # noqa: E402


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("unit", choices=u.UNITS)
    ap.add_argument("--rw", action="append", default=[], help="path the command may write (must exist)")
    ap.add_argument("--drop", action="append", default=[], help="leave this [Service] option out")
    ap.add_argument("--add", action="append", default=[], metavar="KEY=VALUE", help="one more option, after the unit's own")
    ap.add_argument("--plain", action="store_true", help="no hardening at all (baseline)")
    ap.add_argument("--show", action="store_true", help="let systemd-run print runtime, CPU time and memory peak of the unit to stderr")
    ap.add_argument("cmd", nargs="+")
    a = ap.parse_args()

    props = [] if a.plain else u.service_options(a.unit, drop=a.drop) + a.add
    for p in a.rw:
        path = os.path.abspath(p)
        if not os.path.exists(path):
            sys.exit(f"under-unit: --rw {path} does not exist")
        if not a.plain:
            props.append(f"ReadWritePaths={path}")
    name = f"dictate-under-unit-{os.getpid()}"
    child = u.transient(name, props, a.cmd, dict(os.environ), quiet=not a.show)

    def forward(signum, _frame):
        u.systemctl("kill", "--kill-whom=main", f"--signal={signal.Signals(signum).name}", f"{name}.service", capture_output=True)

    signal.signal(signal.SIGTERM, forward)
    signal.signal(signal.SIGINT, forward)
    try:
        rc = child.wait()
    finally:
        u.systemctl("stop", f"{name}.service", capture_output=True)
    sys.exit(rc)


if __name__ == "__main__":
    main()
