#!/usr/bin/env python3
# Copyright (C) 2026 Daniel Kurdoghlian
# Pushing Pixels, Vienna <https://pushingpixels.at>
# SPDX-License-Identifier: GPL-3.0-or-later
"""Zeigt, welche Optionen einer Unit aus systemd/ im User-Manager wirken und was dabei erlaubt bleibt.

    tests/systemd/probe.py UNIT [--drop KEY ...] [--json]

Startet tests/systemd/inside.py zweimal als flüchtige User-Unit, einmal ohne Härtung und einmal unter den
[Service]-Zeilen von systemd/UNIT.service, und stellt die Ergebnisse je Probe nebeneinander. Eine Probe, die ohne
Härtung erlaubt und unter der Unit verweigert ist, zeigt eine wirksame Option; ist sie beide Male verweigert, hat der
User das Recht ohnehin nicht (die Probe kann die Option nicht zeigen); ist sie beide Male erlaubt, wirkt die Option im
User-Manager nicht. Die Proben unter „needed“ müssen unter der Unit erlaubt bleiben, sonst endet das Skript mit 1.

Die Verzeichnis-Direktiven laufen unter dem Wegwerfnamen `dictate-unit-probe` (nie `dictate`, tests/systemd/unit.py);
die Ordner entfernt das Skript wieder. Sonst ändert es nichts.
"""

import argparse
import json
import os
import pathlib
import socket
import subprocess
import sys
import tempfile

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import unit as u  # noqa: E402

NAME = "dictate-unit-probe"


def probe_dirs():
    home = pathlib.Path.home()
    return [home / ".config" / NAME, home / ".local/state" / NAME, home / ".cache" / NAME, pathlib.Path(u.runtime_dir()) / NAME]


def system_dir():
    """Ordner unter /etc, /opt, /srv, /data, /media oder /mnt, den der User beschreiben darf (ProtectSystem=strict
    macht sie nur lesbar), sonst /nonexistent."""
    for top in ("/etc", "/opt", "/srv", "/data", "/media", "/mnt"):
        for d in [pathlib.Path(top), *sorted(pathlib.Path(top).glob("*"))]:
            if d.is_dir() and not d.is_symlink() and os.access(d, os.W_OK):
                return str(d)
    return "/nonexistent"


def run(unit, props, marker, tag, port):
    env = {"PATH": "/usr/bin", "HOME": str(pathlib.Path.home()), "XDG_RUNTIME_DIR": u.runtime_dir()}
    argv = ["/usr/bin/python3", "-I", "-", str(os.getpid()), marker, system_dir(), NAME, unit, str(port)]
    child = u.transient(f"{NAME}-{tag}-{os.getpid()}", props, argv, env, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
    out, _ = child.communicate((u.ROOT / "tests/systemd/inside.py").read_text(), timeout=120)
    results, section = [], ""
    for line in out.splitlines():
        if line.startswith("# "):
            section = line[2:]
        elif ": " in line:
            name, result = line.split(": ", 1)
            results.append((section, name, result))
    return results, out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("unit", choices=u.UNITS)
    ap.add_argument("--drop", action="append", default=[], help="leave this [Service] option out")
    ap.add_argument("--add", action="append", default=[], metavar="KEY=VALUE", help="one more option, after the unit's own")
    ap.add_argument("--json", action="store_true")
    a = ap.parse_args()
    made = []
    for d in probe_dirs():
        if not d.exists():
            d.mkdir(mode=0o700)
            made.append(d)
    try:
        # Lauscher für die TCP-Probe: nur 127.0.0.1, nimmt nichts an (die Verbindung liegt in der Warteschlange)
        with tempfile.NamedTemporaryFile(prefix="dictate-unit-probe-", dir="/tmp") as marker, socket.create_server(("127.0.0.1", 0)) as listener:
            port = listener.getsockname()[1]
            base, _ = run(a.unit, [], marker.name, "plain", port)
            hard, raw = run(a.unit, u.service_options(a.unit, drop=a.drop, dirs=NAME) + a.add, marker.name, "unit", port)
    finally:
        for d in probe_dirs():
            if d.exists() and (d in made or not any(d.iterdir())):
                try:
                    d.rmdir()
                except OSError as e:
                    print(f"probe: could not remove {d}: {e}", file=sys.stderr)
    under = {name: result for _, name, result in hard}
    rows, broken = [], []
    for section, name, b in base:
        h = under.get(name, "(no answer)")
        if section == "needed":
            verdict = "kept" if h.startswith("allowed") else ("not available either way" if not b.startswith("allowed") else "BROKEN")
            if verdict == "BROKEN":
                broken.append(name)
        elif h.startswith("denied") and b.startswith("allowed"):
            verdict = "takes effect"
        elif h.startswith("denied"):
            verdict = "denied either way"
        elif b.startswith("allowed"):
            verdict = "allowed either way"
        else:
            verdict = "allowed only under the unit"
        rows.append({"section": section, "probe": name, "without": b, "unit": h, "verdict": verdict})
    if len(hard) < len(base):
        print(raw)
    if a.json:
        print(json.dumps(rows, indent=1))
    else:
        for r in rows:
            print(f"{r['probe']}\n    without: {r['without']}\n    unit:    {r['unit']}\n    -> {r['verdict']}")
    if broken:
        sys.exit(f"probe: needed under {a.unit}.service but denied: {', '.join(broken)}")


if __name__ == "__main__":
    main()
