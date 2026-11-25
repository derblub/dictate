# Copyright (C) 2026 Daniel Kurdoghlian
# Pushing Pixels, Vienna <https://pushingpixels.at>
# SPDX-License-Identifier: GPL-3.0-or-later
"""Gemeinsame Teile der Werkzeuge unter tests/systemd: liest die Härtung aus systemd/<unit>.service (die einzige
Quelle, dieselben Zeilen wie im installierten Dienst) und startet Befehle als flüchtige Unit im User-Manager.

Regeln für jeden flüchtigen Lauf (docs/systemd-hardening.md, „Method“):

- Die Verzeichnis-Direktiven der Unit (`RuntimeDirectory=dictate` usw.) gehen nie unter ihrem echten Namen an eine
  flüchtige Unit: systemd würde die Ordner des laufenden Dienstes anlegen, umstellen oder beim Ende entfernen.
  `service_options` lässt sie weg (`dirs=None`) oder ersetzt den Namen (`dirs="dictate-unit-probe"`).
- Eine flüchtige Unit erbt die Umgebung des User-Managers, also die echte Sitzung (WAYLAND_DISPLAY, NIRI_SOCKET).
  `transient` startet den Befehl darum über `env -i` mit genau der Umgebung des Aufrufers.
- Nichts wird nach ~/.config/systemd kopiert, kein daemon-reload.
"""

import os
import pathlib
import re
import subprocess

ROOT = pathlib.Path(__file__).resolve().parents[2]
UNITS = ("dictate", "dictate-ui")
# [Service]-Zeilen, die keine Härtung sind (Start, Neustart, Umgebung des echten Dienstes)
NOT_HARDENING = {"Type", "ExecStart", "Restart", "RestartSec", "Environment"}
DIRECTORY = re.compile(r"^(Runtime|State|Cache|Configuration|Logs)Directory$")


def _uid():
    return os.getuid()


def runtime_dir():
    return f"/run/user/{_uid()}"


def manager_env():
    """Umgebung für systemd-run/systemctl --user, unabhängig von der Umgebung des Aufrufers (Tests biegen
    XDG_RUNTIME_DIR um und entfernen den Sitzungs-Bus)."""
    return {
        "PATH": "/usr/bin",
        "XDG_RUNTIME_DIR": runtime_dir(),
        "DBUS_SESSION_BUS_ADDRESS": f"unix:path={runtime_dir()}/bus",
    }


def specifiers():
    home = os.path.expanduser("~")
    return {"%h": home, "%t": runtime_dir(), "%E": f"{home}/.config", "%S": f"{home}/.local/state", "%C": f"{home}/.cache"}


def expand(value):
    for spec, real in specifiers().items():
        value = value.replace(spec, real)
    return value


def service_lines(unit):
    """`(Schlüssel, Wert)` je Zeile des Abschnitts [Service], in der Reihenfolge der Datei."""
    assert unit in UNITS, unit
    section, out = None, []
    for raw in (ROOT / "systemd" / f"{unit}.service").read_text().splitlines():
        line = raw.strip()
        if not line or line.startswith(("#", ";")):
            continue
        if line.startswith("["):
            section = line
            continue
        if section == "[Service]":
            key, _, value = line.partition("=")
            out.append((key.strip(), value.strip()))
    return out


def service_options(unit, drop=(), dirs=None):
    """Die Härtung der Unit als `Schlüssel=Wert` für `systemd-run -p`, Platzhalter aufgelöst.

    `drop`: Schlüssel, die fehlen sollen. `dirs`: None lässt die Verzeichnis-Direktiven weg, ein Name ersetzt den
    Ordnernamen `dictate` (nie der echte Name, siehe Modulkopf).
    """
    assert dirs != "dictate"
    out = []
    for key, value in service_lines(unit):
        if key in NOT_HARDENING or key in drop:
            continue
        if DIRECTORY.match(key):
            if dirs is None:
                continue
            assert value == "dictate", f"{key}={value}"
            value = dirs
        elif key.endswith(("DirectoryMode", "DirectoryPreserve")):
            if dirs is None:
                continue
        out.append(f"{key}={expand(value)}")
    return out


def transient(name, props, argv, env, quiet=True, **kw):
    """Startet `argv` als flüchtige Unit `name` mit genau der Umgebung `env` und wartet auf ihr Ende
    (`subprocess.Popen`, stdin/stdout/stderr des Aufrufers, wenn `kw` nichts anderes sagt). Ohne `quiet` nennt
    systemd-run am Ende Laufzeit, CPU-Zeit und Speicherspitze der Unit auf stderr."""
    cmd = ["/usr/bin/systemd-run", "--user", "--wait", "--pipe", "--collect", f"--unit={name}"]
    if quiet:
        cmd.append("--quiet")
    for p in props:
        cmd += ["-p", p]
    # Rückfall, falls der Aufrufer hart beendet wird und die Unit nicht mehr stoppen kann
    cmd += ["-p", "RuntimeMaxSec=3600", f"--working-directory={os.getcwd()}"]
    cmd += ["/usr/bin/env", "-i", *[f"{k}={v}" for k, v in env.items()], *argv]
    return subprocess.Popen(cmd, env=manager_env(), **kw)


def systemctl(*args, **kw):
    return subprocess.run(["/usr/bin/systemctl", "--user", *args], env=manager_env(), **kw)
