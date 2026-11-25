# Copyright (C) 2026 Daniel Kurdoghlian
# Pushing Pixels, Vienna <https://pushingpixels.at>
# SPDX-License-Identifier: GPL-3.0-or-later
"""Läuft in einer flüchtigen User-Unit (stdin von `python3 -`, gestartet von tests/systemd/probe.py): eine Zeile je
Probe, „name: allowed|denied (Detail)“.

Die Proben unter „options“ tun, was die Unit nie tut, und zeigen, ob eine Option im User-Manager wirkt. Die Proben
unter „needed“ tun, was die Unit braucht (je Unit verschieden), und müssen erlaubt bleiben. Nichts ändert das System: geschrieben wird nur unter
Wegwerfnamen, die wieder entfernt werden; Geräte werden nur geöffnet und per ioctl gefragt, nie gelesen; Sockets nur
verbunden, nie beschrieben. Den Wayland- und den niri-Socket der echten Sitzung fasst keine Probe an.
"""

import ctypes
import fcntl
import glob
import os
import socket
import sys

libc = ctypes.CDLL(None, use_errno=True)
# PID eines Prozesses desselben Users außerhalb der Unit (der Aufrufer)
OUTSIDE_PID = int(sys.argv[1])
# Datei, die der Aufrufer im /tmp des Rechners angelegt hat
HOST_MARKER = sys.argv[2]
# Ordner außerhalb von /home, /tmp und /run, den der User beschreiben darf (sonst /nonexistent)
SYSTEM_DIR = sys.argv[3]
# Wegwerfname der Ordner, die der Aufrufer bzw. systemd für diesen Lauf angelegt hat
NAME = sys.argv[4]
# `dictate` oder `dictate-ui`: was die Unit braucht, unterscheidet sich
UNIT = sys.argv[5]
DAEMON = UNIT == "dictate"
# Port auf 127.0.0.1, auf dem der Aufrufer lauscht
LISTEN_PORT = int(sys.argv[6])
HOME = os.path.expanduser("~")
RUNTIME = f"/run/user/{os.getuid()}"
TAG = f".dictate-unit-probe-{os.getpid()}"


def probe(name, fn):
    """Führt `fn` in einem eigenen Kindprozess aus: eine Probe, die den Prozess ändert (neuer Namensraum,
    personality), ändert die nächste nicht; ein Abbruch durch den Systemaufruf-Filter trifft nur sie."""
    r, w = os.pipe()
    pid = os.fork()
    if pid == 0:
        os.close(r)
        try:
            detail = fn()
            line = f"allowed{f' ({detail})' if detail else ''}"
        except OSError as e:
            line = f"denied ({e.errno} {os.strerror(e.errno) if e.errno else e})"
        os.write(w, line.encode())
        os._exit(0)
    os.close(w)
    with os.fdopen(r) as f:
        line = f.read()
    os.waitpid(pid, 0)
    print(f"{name}: {line or 'no answer (the probe was killed)'}", flush=True)


def check(rc):
    if rc != 0:
        err = ctypes.get_errno()
        raise OSError(err, os.strerror(err))


def write_remove(path):
    with open(path, "w") as f:
        f.write("x")
    os.remove(path)


def mkdir_remove(path):
    os.mkdir(path)
    os.rmdir(path)


def own_cgroup():
    rel = open("/proc/self/cgroup").read().strip().split("::", 1)[1]
    return "/sys/fs/cgroup" + rel


def sock(family, kind=socket.SOCK_DGRAM):
    socket.socket(family, kind).close()


def connect(path):
    s = socket.socket(socket.AF_UNIX)
    try:
        s.connect(path)
    finally:
        s.close()


def hosts_tmp():
    if not os.path.exists(HOST_MARKER):
        raise OSError(2, "the host's /tmp marker is not there")


def rwx_map():
    libc.mmap.restype = ctypes.c_void_p
    addr = libc.mmap(None, 4096, 7, 0x22, -1, 0)
    if addr in (None, ctypes.c_void_p(-1).value):
        err = ctypes.get_errno()
        raise OSError(err, os.strerror(err))
    libc.munmap(ctypes.c_void_p(addr), 4096)


def realtime():
    class Param(ctypes.Structure):
        _fields_ = [("prio", ctypes.c_int)]

    check(libc.sched_setscheduler(0, 1, ctypes.byref(Param(1))))  # SCHED_FIFO 1
    check(libc.sched_setscheduler(0, 0, ctypes.byref(Param(0))))


def personality():
    if libc.personality(0x0400000) == -1:  # READ_IMPLIES_EXEC
        err = ctypes.get_errno()
        raise OSError(err, os.strerror(err))


def root_processes():
    if not os.path.exists("/proc/1"):
        raise OSError(2, "PID 1 not visible")
    return "PID 1 visible"


def outside_environ():
    open(f"/proc/{OUTSIDE_PID}/environ", "rb").read(1)


def open_close(path, flags):
    os.close(os.open(path, flags))


def key_state():
    """Wie `ui::keys`: Gerät nur öffnen und den Tastenzustand per EVIOCGKEY fragen, nie lesen."""
    devices = sorted(glob.glob("/dev/input/event*"))
    if not devices:
        raise OSError(2, "no /dev/input/event*")
    buf = bytearray(96)
    eviocgkey = (2 << 30) | (len(buf) << 16) | (ord("E") << 8) | 0x18
    fd = os.open(devices[0], os.O_RDONLY | os.O_NONBLOCK)
    try:
        fcntl.ioctl(fd, eviocgkey, buf)
    finally:
        os.close(fd)
    return f"EVIOCGKEY on 1 of {len(devices)} devices"


def journal():
    files = glob.glob("/var/log/journal/*/system.journal")
    if not files:
        raise OSError(2, "no system journal")
    open(files[0], "rb").read(8)


def modules_dir():
    return f"{len(os.listdir('/usr/lib/modules'))} entries"


def tcp_connect():
    s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    s.settimeout(3)
    try:
        s.connect(("127.0.0.1", LISTEN_PORT))
    finally:
        s.close()


def tcp():
    # nur der Socket: kein Verbindungsaufbau nach außen
    sock(socket.AF_INET, socket.SOCK_STREAM)


def rename_in(directory):
    a, b = f"{directory}/{TAG}", f"{directory}/{TAG}.2"
    open(a, "w").close()
    os.rename(a, b)
    os.remove(b)


print("# options")
probe("write in the home directory (ProtectHome=read-only)", lambda: write_remove(f"{HOME}/{TAG}"))
probe(f"write in {SYSTEM_DIR} (ProtectSystem=strict)", lambda: write_remove(f"{SYSTEM_DIR}/{TAG}"))
probe("write in the runtime directory outside its own (ProtectHome=read-only)", lambda: write_remove(f"{RUNTIME}/{TAG}"))
probe("the host's /tmp (PrivateTmp)", hosts_tmp)
probe("new user namespace (RestrictNamespaces)", lambda: check(libc.unshare(0x10000000)))
probe("AF_PACKET socket (RestrictAddressFamilies)", lambda: sock(socket.AF_PACKET, socket.SOCK_RAW))
probe("AF_BLUETOOTH socket (RestrictAddressFamilies)", lambda: sock(socket.AF_BLUETOOTH))
if not DAEMON:
    probe("AF_INET socket (RestrictAddressFamilies)", tcp)
    probe("AF_NETLINK socket (RestrictAddressFamilies)", lambda: sock(socket.AF_NETLINK, socket.SOCK_RAW))
probe("writable and executable memory (MemoryDenyWriteExecute)", rwx_map)
probe("SCHED_FIFO (RestrictRealtime)", realtime)
probe("sched_setaffinity (SystemCallFilter=~@resources)", lambda: os.sched_setaffinity(0, os.sched_getaffinity(0)))
probe("personality READ_IMPLIES_EXEC (LockPersonality)", personality)
probe("a cgroup below its own (ProtectControlGroups)", lambda: mkdir_remove(own_cgroup() + "/dictate-unit-probe"))
probe("/usr/lib/modules (ProtectKernelModules)", modules_dir)
probe("/dev/kmsg (ProtectKernelLogs)", lambda: open_close("/dev/kmsg", os.O_RDONLY | os.O_NONBLOCK))
probe("root's processes in /proc (ProtectProc=invisible, not used)", root_processes)
probe("/proc/<pid>/environ of a process outside the unit (user namespace)", outside_environ)
if DAEMON:
    probe("TCP connection to 127.0.0.1 (IPAddressDeny, not used)", tcp_connect)
probe("/dev/nvidia-uvm-tools, not needed (DevicePolicy, not used)", lambda: open_close("/dev/nvidia-uvm-tools", os.O_RDWR))
print("# needed")
probe("read the home directory", lambda: os.listdir(HOME) and None)
probe("session bus socket (connect only)", lambda: connect(f"{RUNTIME}/bus"))
probe("PulseAudio socket (connect only)", lambda: connect(f"{RUNTIME}/pulse/native"))
probe("system bus socket (connect only)", lambda: connect("/run/dbus/system_bus_socket"))
probe("/dev/nvidiactl (GPU)", lambda: open_close("/dev/nvidiactl", os.O_RDWR))
probe("write in its runtime directory", lambda: write_remove(f"{RUNTIME}/{NAME}/{TAG}"))
probe("write in the state directory", lambda: write_remove(f"{HOME}/.local/state/{NAME}/{TAG}"))
if DAEMON:
    probe("write and rename in the configuration directory (Config::save)", lambda: rename_in(f"{HOME}/.config/{NAME}"))
    probe("AF_INET socket (AI providers)", tcp)
    probe("AF_NETLINK socket (ip and ss of the Linux diagnosis)", lambda: sock(socket.AF_NETLINK, socket.SOCK_RAW))
    probe("/dev/nvidia-uvm (CUDA)", lambda: open_close("/dev/nvidia-uvm", os.O_RDWR))
    probe("Docker socket (connect only; group docker)", lambda: connect("/var/run/docker.sock"))
    probe("system journal (group access)", journal)
    probe("/var/lib/systemd/pstore (ACL of the author's machine)", lambda: f"{len(os.listdir('/var/lib/systemd/pstore'))} entries")
    probe("/proc/<pid>/fd of a process outside the unit (ss names the user's processes)", lambda: f"{len(os.listdir(f'/proc/{OUTSIDE_PID}/fd')) > 0}")
else:
    probe("write in the cache directory", lambda: write_remove(f"{HOME}/.cache/{NAME}/{TAG}"))
    probe("key state of an input device (group input; ioctl only)", key_state)
