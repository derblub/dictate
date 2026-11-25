# Hardening of the systemd user units

`systemd/dictate.service` and `systemd/dictate-ui.service` carry systemd's sandboxing options as far as dictate still
works under them. This page says which options are in, which are out and why, and how each one was measured
(2026-11-25, systemd 262, Arch Linux, NVIDIA driver 620, niri).

| | Before | After |
|---|---|---|
| `dictate.service` (`systemd-analyze --user security --offline=yes`) | 9.2 UNSAFE | 5.9 MEDIUM |
| `dictate-ui.service` | 9.2 UNSAFE | 1.4 OK |

The installed units before the change scored 9.4 when asked from the running user manager; the offline figure is the
one that can be repeated on the unit files alone.

## What the hardening is, and what it is not

It limits what a bug in dictate can reach: which kernel interfaces, and for the overlay also which files. It is no
boundary against code running in dictate. The daemon keeps the network (AI providers), the session bus, the GPU and
the whole home directory. The overlay keeps the compositor's sockets (it types and pastes text), the session bus and
the key state of the input devices. The exposure level of `systemd-analyze` counts options, not this.

## The two units

Both units get the options that need no namespace:

`NoNewPrivileges`, `UMask=0077`, `LockPersonality`, `RestrictSUIDSGID`, `SystemCallArchitectures=native`,
`RestrictAddressFamilies`, `RestrictNamespaces`, `KeyringMode=private`, `RestrictRealtime` and a system call filter
(`@system-service` without `@privileged` and `@resources`, answering `EPERM`).

**`dictate.service`** stops there, with two additions: `MemoryDenyWriteExecute=yes`, and four calls allowed again in
the filter for the Linux diagnosis. The daemon starts every probe with a low CPU and I/O priority (`setpriority`,
`ioprio_set`) and does not start it if it cannot; `ip` and `ping` drop their capabilities and their user ID when they
start (`capset`, `setuid`) and end if they may not. Address families: `AF_UNIX AF_INET AF_INET6 AF_NETLINK`.

**`dictate-ui.service`** also gets the file system and kernel options: `ProtectSystem=strict`,
`ProtectHome=read-only`, `PrivateTmp`, `PrivateUsers`, `ProtectKernelTunables`, `ProtectKernelModules`,
`ProtectKernelLogs`, `ProtectControlGroups`, `ProtectClock`, `ProtectHostname` and an empty `CapabilityBoundingSet`.
Address families: `AF_UNIX` only, the overlay has no network. It may write three directories that systemd creates:
its state directory (`~/.local/state/dictate`, where it watches the colors Noctalia writes), its cache directory
(`~/.cache/dictate`, which `XDG_CACHE_HOME` points the graphics driver, GTK and fontconfig to) and the runtime
directory `$XDG_RUNTIME_DIR/dictate` with its control socket. The runtime directory is shared with the daemon and the
settings window, so it is kept when the unit stops (`RuntimeDirectoryPreserve=yes`).

## Left out, and why

| Option | Unit | Why it is out |
|---|---|---|
| `ProtectSystem`, `ProtectHome`, `PrivateTmp`, `ProtectKernelTunables`, `ProtectKernelModules`, `ProtectKernelLogs`, `ProtectControlGroups` | daemon | They break the Linux diagnosis. Its programs run as children of the daemon and inherit the mount namespace: `findmnt` reported a network share as `ro,nosuid` that is mounted `rw`, and listed it twice; `lsblk` showed `/var/tmp` as an extra mount point of the root partition. A diagnosis that reports the sandbox instead of the system is wrong. |
| `PrivateUsers`, and with it `ProtectClock`, `ProtectHostname`, `CapabilityBoundingSet=` | daemon | In a user manager each of these puts the service into a user namespace (measured one by one; `CapabilityBoundingSet=` alone fails with 218/CAPABILITIES). From inside it `ss -Htulpn` no longer names the user's own processes behind a listening port, because `/proc/<pid>/fd` of a process outside the namespace is not readable. |
| `SystemCallFilter=~@privileged @resources` without exceptions | daemon | `ip -j addr` and `ip -j route` ended with 1 and no output, `ping` with 255: both call `capset`, `ping` also `setuid`. And no probe started at all: 8 of the 19 tests in `tests/daemon_diagnose.rs` failed under the filter, because the daemon lowers each probe's priority with `setpriority` and `ioprio_set` first. With these four calls allowed again the programs answer as without the filter and the tests pass. |
| `MemoryDenyWriteExecute` | overlay | The NVIDIA OpenGL driver maps parts of `libnvidia-glcore` and `libGLX_nvidia` writable and executable: two such mappings in the overlay, also when it ran under the unit in the live session. In the windowless driver check the driver fell back and still drew under the option, but nothing shows that for the overlay's own path (GTK on Wayland), so the option stays out. The daemon has no such mapping (CUDA compiles for the GPU, not for the CPU) and keeps the option. |
| `PrivateDevices` | both | It hides the NVIDIA device nodes and `/dev/input`: tried on the overlay unit, `probe.py` reports `/dev/nvidiactl` and the key state as broken. |
| `PrivateNetwork`, `IPAddressDeny` | both | The daemon needs the network. For the overlay `RestrictAddressFamilies=AF_UNIX` already removes it. `IPAddressDeny=any` has no effect in a user manager: tried on the daemon unit, a TCP connection to 127.0.0.1 still went through. |
| `DevicePolicy`, `DeviceAllow` | both | No effect in a user manager: with `DevicePolicy=closed` on either unit `/dev/nvidia-uvm-tools`, which dictate does not need, could still be opened. |
| `ProtectProc=invisible` | both | No effect in a user manager (tried on the overlay unit: PID 1 stays visible), and the diagnosis reads `/proc`. |
| `ProtectHome=tmpfs` or `yes` | overlay | The overlay reads the GTK settings, fonts and the color file from the home directory. Read-only is enough to stop writes. |
| `MemoryMax`, `TasksMax` | both | Not security options (no effect on the exposure level). The daemon's memory follows the speech model the user picks and is several times higher without a GPU; a limit that fits one setup ends dictation on another. Peaks measured here: 546 MB for the daemon test below, 970 MB for the installed daemon over two days, 81 to 88 MB for the overlay. |

The daemon therefore keeps write access to the home directory. See [Open](#open) for the way out.

## Method

Three tools under `tests/systemd/` read the `[Service]` lines from the unit files themselves, so what is measured is
what is installed:

- `probe.py UNIT` runs the same probes twice as a transient user unit, without hardening and under the unit's options.
  A probe that is allowed without and denied under the unit shows an option that takes effect; one that is allowed both
  times shows an option without effect in a user manager; the probes under "needed" must stay allowed, or the script
  fails. Probes only open devices and connect to sockets, never read input devices and never touch the session's
  Wayland or niri socket.
- `programs.py` runs every program dictate starts as a child, once without and once under the unit that starts it:
  one command line per program of the diagnosis catalog under the daemon's options, and read-only forms of the
  overlay's short commands (`pactl`, `playerctl`, `systemctl`, `loginctl`, `noctalia msg`, logind's `CanReboot` for
  the polkit decision) under the overlay's. Exit status, standard error and output must match once digits are masked
  and lines sorted; a command whose text differs runs a third time without hardening to tell a change of the system
  from an effect of the unit. It starts the programs directly, not through the daemon: what the daemon does around a
  probe is covered by running its tests under the unit (below).
- `under-unit.py UNIT -- COMMAND` runs any command under a unit's options with exactly the caller's environment. The
  Wayland tests use it for the overlay (`DICTATE_UI_UNDER_UNIT=1 tests/wayland/run.sh`).
- `gl-check.py` loads the machine's OpenGL driver without a window (an EGL context on the device), compiles a shader,
  draws and reads a pixel back. The Wayland tests render in software, so this is what shows the graphics driver under
  the overlay's system call filter, user namespace and read-only home directory.

Rules for every transient run: the directory options never go to a transient unit under the name `dictate` (systemd
would create, chmod or remove the directories of the running services; the tools use `dictate-unit-probe` or
`ReadWritePaths` on test folders), the command gets the caller's environment and not the user manager's (which holds
the real session), and nothing is copied to `~/.config/systemd`.

## Results

### Options that take effect (`probe.py`)

| Probe | Daemon unit | Overlay unit |
|---|---|---|
| Write in the home directory | allowed (no file system options) | denied, read-only |
| Write in a user-writable folder below `/etc` | allowed | denied, read-only |
| Write in `$XDG_RUNTIME_DIR` outside `dictate/` | allowed | denied, read-only |
| See a file in the host's `/tmp` | allowed | denied |
| New user namespace | denied | denied |
| `AF_BLUETOOTH` socket | denied | denied |
| `AF_INET` and `AF_NETLINK` sockets | allowed (needed) | denied |
| Writable and executable memory | denied | allowed (NVIDIA OpenGL) |
| `sched_setaffinity` | denied | denied |
| `personality(READ_IMPLIES_EXEC)` | denied | denied |
| A control group below its own | allowed | denied, read-only |
| `/usr/lib/modules` | allowed | denied |
| `/proc/<pid>/environ` of a process outside the unit | allowed | denied (user namespace) |
| `AF_PACKET`, `SCHED_FIFO`, `/dev/kmsg` | denied either way | denied either way |

`ProtectClock`, `ProtectKernelLogs`, `ProtectHostname`, `RestrictRealtime` and the empty capability set guard what an
unprivileged user cannot do anyway. In the overlay unit they cost nothing and keep the unit right if it ever runs with
more rights; `KeyringMode=private` has no probe.

### What stays allowed

Under both units: reading the home directory, the session bus, the PulseAudio socket, the system bus,
`/dev/nvidiactl`, the unit's runtime and state directory. Under the daemon unit also the configuration directory
(write and rename, as the settings are saved), `AF_INET` and `AF_NETLINK`, `/dev/nvidia-uvm`, the Docker socket, the
system journal and `/proc/<pid>/fd` of the user's processes. Under the overlay unit also the cache directory and the
key state of an input device (`EVIOCGKEY`; the group `input` still applies inside the user namespace).

### Programs started by dictate (`programs.py`)

All 22 command lines of the diagnosis catalog under the daemon unit and all 11 short commands under the overlay unit
answered as without hardening: same exit status and standard error, same output apart from counters, timestamps and
measured values. `resolvectl status` ended with 1 both ways (systemd-resolved does not run on this machine), so it was
not exercised. Three controls show that the comparison catches the failures above: with `ProtectClock=yes` added to
the daemon unit it reports `ss -Htulpn` as different (the user namespace), with `ProtectSystem=strict` also `findmnt`
and `lsblk`, and with `capset` and `setuid` removed from the filter `ip` and `ping`.

### The shared runtime directory

Two transient units with `RuntimeDirectory=dictate-unit-probe` and `RuntimeDirectoryPreserve=yes`, each with a
listening socket in it: after one unit stopped, the directory and the other unit's socket were still there and the
socket still answered; a process outside both units could create a file in it (as the settings window does).

### dictate under the hardening

The daemon, under the options of `dictate.service` (`under-unit.py dictate -- <test binary> --ignored`):

| Run | What it shows | Result |
|---|---|---|
| `dictate transcribe` on a test recording, with debug log | the model loads on the GPU | whisper.cpp logs `ggml_cuda_init: found 1 CUDA devices`, `CUDA0 total size = 573.45 MB`, `using CUDA0 backend` |
| `engine_real` (5 tests) | recognition and voice activity detection on the GPU | passed in 4.4 s, memory peak 390 MB |
| `daemon_pulse_real` (2 tests) | end to end like holding `Mod+E`: a virtual PulseAudio source plays recordings, a daemon captures them, recognizes on the GPU and answers on its socket | passed in 70 s ("Test 1, 2, 3" from the quiet recording), memory peak 546 MB |
| `daemon_diagnose`, `daemon_palette`, `daemon_actions`, `daemon_ai`, `ai_http`, `diagnose_ai_http`, `palette_ai_http` | the daemon's own paths around child processes and the network: starting, throttling, stopping and reaping probes (fake programs), the HTTP client against local mock servers | all pass (134 tests); before the four calls were allowed again, 8 of 19 in `daemon_diagnose` failed |
| the library's unit tests `diagnose::`, `http::`, `palette::` | the same paths at module level | 574 pass |

The overlay, under the options of `dictate-ui.service`:

| Run | What it shows | Result |
|---|---|---|
| `DICTATE_UI_UNDER_UNIT=1 tests/wayland/run.sh` | the whole Wayland suite in a nested niri with `dictate-ui` as a transient unit: the overlay appears, hold and release, text output through the input method, the clipboard and the virtual keyboard into a text field and terminals, the panels, the command palette with its short commands and niri actions, the lock through a (fake) logind | WAYLAND_RESULT |
| `gl-check.py` without hardening, under the unit, and under the unit plus `MemoryDenyWriteExecute` | the NVIDIA OpenGL driver under the unit | all three draw (renderer "NVIDIA GeForce RTX 3060", the pixel read back is the drawn color); the shader cache lands below `XDG_CACHE_HOME` (2 files); one writable and executable mapping without the option, none with it |

| the overlay binary as a transient unit with the unit's 30 options in the live niri session, in place of the stopped `dictate-ui.service` for a few seconds (2026-11-25, agreed beforehand; `dictate-ui toggle`, then `cancel`) | the overlay itself on the real compositor with the real driver | connected to the daemon, renderer ready 490 ms after start (509 ms for the unhardened service right after), logind watched, overlay shown 648 ms after `toggle`, cancelled, no warning or error in the log; the NVIDIA libraries mapped, shader and font caches written below `~/.cache/dictate`, memory peak 88 MB; the regular service started again and reconnected |

The nested session renders in software and reads the key state from a file. The graphics driver is therefore shown by
`gl-check.py` and the live start, and the key state of a real input device (`EVIOCGKEY` under the unit) by `probe.py`.
Not shown live: holding the key, text output into a real application and the panels under the hardened unit; these
ran only in the nested session.

## The settings window is started by niri

The overlay used to start `dictate-settings` as a child process (the "Settings" button in a panel). A child inherits
the unit's sandbox: it could not download models (read-only home), and a browser opened from its About page would have
run without network. The button now asks niri to start the window, the same way the command palette already did, so
the window runs outside the sandbox like one started from the shortcut. Applications and terminals from the palette
were started by niri before; only the short commands listed above run inside the overlay's unit.

## If your setup needs more

Add a drop-in with `systemctl --user edit dictate-ui.service`; nothing in the unit has to be copied.

- The overlay finds its color file through a symbolic link that leaves `~/.local/state/dictate`, or your XDG
  directories are links to another disk: reading works everywhere, writing only in the three directories above. Add
  `ReadWritePaths=` for a path the overlay must write.
- A graphics driver that fails under an option: remove it with an empty assignment for list options (for example
  `SystemCallFilter=` followed by the filter you want) or `=no` for switches, and please report it.
- To see what a unit may do on your system: `tests/systemd/probe.py dictate-ui` from a clone of the repository.

## Open

- **File system options for the daemon.** The daemon keeps write access to the home directory because the diagnosis
  starts its programs as children, which must see the real system. The way out is to start those programs outside the
  daemon's sandbox (as transient units of the user manager, for example) and then give `dictate.service` the options
  the overlay has. That changes how the diagnosis starts, limits and stops programs, which is security-reviewed code
  (the catalog is the only gate, nothing runs through a shell), so it needs a design and a security review of its own.
- **`MemoryDenyWriteExecute` for the overlay**, once it is shown that GTK draws with the NVIDIA driver's fallback.
- **Live use under the installed units** (hold to speak, text output into applications, panels) after the units are
  installed on the author's machine.

## Packaging

The AUR package installs the unit files from the release tarball (`packaging/aur/PKGBUILD` rewrites only the path of
the binaries), so the hardened units ship with the first release after 0.1.0; the package itself needs no change.
`dictate 0.1.0-1` keeps the units without hardening.
