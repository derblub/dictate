#!/usr/bin/env python3
# Copyright (C) 2026 Daniel Kurdoghlian
# Pushing Pixels, Vienna <https://pushingpixels.at>
# SPDX-License-Identifier: GPL-3.0-or-later
"""Erzeugt THIRD-PARTY-LICENSES.txt (U2, PLAN-lizenz.md) aus Cargo.lock und den Quellen im Cargo-Registry-Cache.

Aufruf: scripts/third-party-licenses.py          Datei und licenses/rust-std-*.html schreiben
        scripts/third-party-licenses.py --check  nur vergleichen, Exit 1 bei Abweichung (Schritt `licenses` in
                                                 scripts/check.sh)

Erfasst wird, was in die Binaries geht: alle Pakete im normalen Abhängigkeitsgraphen für x86_64 Linux mit den
Vorgabe-Features (ohne Dev- und Build-Abhängigkeiten; Proc-Macros und ihre Abhängigkeiten bleiben drin, weil der
erzeugte Code im Binary steht), dazu die Rust-Standardbibliothek der gepinnten Toolchain, der in whisper-rs-sys
gebündelte Quelltext von whisper.cpp/ggml und die EFF-Wortliste. Je Paket werden alle Lizenzdateien wiedergegeben, die
es mitbringt (auch verschachtelte, ohne Test-, Doku- und Beispielordner); Pakete ohne eigene Datei bekommen sie aus
licenses/upstream/ (Herkunft in licenses/upstream/SOURCES.md). Fehlt beides, bricht das Skript ab: eine neue
Abhängigkeit braucht dann eine bewusste Entscheidung. Die Ausgabe ist deterministisch (keine Pfade, kein Datum),
damit --check nicht wackelt. Nur lesend gegen den Registry-Cache, kein Netz (cargo metadata --offline --locked).
"""

import hashlib
import json
import os
import re
import subprocess
import sys
import tomllib

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT = os.path.join(ROOT, "THIRD-PARTY-LICENSES.txt")
LIC_DIR = os.path.join(ROOT, "licenses")
UPSTREAM_DIR = os.path.join(LIC_DIR, "upstream")
TARGET = "x86_64-unknown-linux-gnu"

# Pakete ohne eigene Lizenzdatei im veröffentlichten Crate: Ordner unter licenses/upstream/ (Herkunft je Ordner in
# SOURCES.md). Ändert sich eine Version, schlägt das Skript fehl, bis der Ordner zur neuen Version passt.
UPSTREAM = {
    ("accesskit", "0.25.1"): "accesskit-0.25.1",
    ("ecolor", "0.37.0"): "egui-0.37.0",
    ("eframe", "0.37.0"): "egui-0.37.0",
    ("egui", "0.37.0"): "egui-0.37.0",
    ("egui-wgpu", "0.37.0"): "egui-0.37.0",
    ("egui-winit", "0.37.0"): "egui-0.37.0",
    ("egui_glow", "0.37.0"): "egui-0.37.0",
    ("emath", "0.37.0"): "egui-0.37.0",
    ("epaint", "0.37.0"): "egui-0.37.0",
    ("gl", "0.14.0"): "gl-0.14.0",
    ("gtk4-layer-shell", "0.8.1"): "gtk4-layer-shell-0.8.1",
    ("gtk4-layer-shell-sys", "0.6.1"): "gtk4-layer-shell-sys-0.6.1",
    ("profiling", "1.0.18"): "profiling-1.0.18",
    ("whisper-rs-sys", "0.15.0"): "whisper-rs-sys-0.15.0",
}
# Pakete unter der Lizenz von dictate selbst: der Text steht in LICENSE, keine eigene Datei nötig
SAME_AS_DICTATE = {"niri-ipc": "GPL-3.0-or-later"}
# Feste Hinweise je Paket in Abschnitt 4 (Namensnennung, die nicht aus einer Lizenzdatei des Crates kommt).
# webpki-roots: die CCADB verlangt in ihren Data Usage Terms die Nennung (O2, PLAN-lizenz.md U3d). Fehlt ein Paket im
# Graphen, bricht das Skript ab, damit kein Hinweis veraltet.
NOTES = {
    "webpki-roots": (
        "contains data derived from the Common CA Database (CCADB),\n"
        "https://www.ccadb.org/, used under the CCADB Data Usage Terms\n"
        "(CDLA-Permissive-2.0, text in section 5):\n"
        "https://www.ccadb.org/rootstores/usage#ccadb-data-usage-terms"
    ),
}
# Ordner für den YaRN-Hinweis in ggml (MIT-Text aus dem genannten Repository)
YARN_DIR = "yarn"

LICENSE_FILE = re.compile(r"^(licen[cs]e|copying|notice|copyright|unlicense|patents)", re.I)
# Verschachtelte Lizenzdateien in diesen Ordnern betreffen nicht, was kompiliert wird (Testdaten, Bilder der Doku)
SKIP_DIRS = {"tests", "test", "examples", "benches", "docs", "doc", "assets", "target", ".git", ".github"}

# Bei reinen OR-Ausdrücken die erste passende Lizenz dieser Liste (alle mit GPL-3.0-or-later vereinbar)
PREFERENCE = [
    "MIT",
    "Apache-2.0",
    "Apache-2.0 WITH LLVM-exception",
    "BSD-3-Clause",
    "BSD-2-Clause",
    "ISC",
    "Zlib",
    "0BSD",
    "Unlicense",
    "Unicode-3.0",
    "MPL-2.0",
    "CDLA-Permissive-2.0",
    "GPL-3.0-or-later",
]

EFF_SECTION = """\
The secret filter of text actions (src/actions/secrets.rs) embeds the unmodified
"EFF Large Wordlist for Passphrases" (assets/wordlists/eff_large_wordlist.txt,
SHA-256 addd35536511597a02fa0a9ff1e5284677b8883b83e986e43f15a3db996b903e)
into the dictate binary.

  Title:   EFF Large Wordlist for Passphrases
  Author:  Electronic Frontier Foundation (list by Joseph Bonneau)
  Source:  https://www.eff.org/dice
           https://www.eff.org/files/2016/07/18/eff_large_wordlist.txt
  License: Creative Commons Attribution 4.0 International (CC BY 4.0)
           https://creativecommons.org/licenses/by/4.0/
           EFF's copyright policy (https://www.eff.org/copyright) places the
           original material on its website under CC BY 4.0; when the list was
           published in July 2016, the policy named CC BY 3.0 United States.
  Changes: none (the file is used verbatim).
"""

HEADER = """\
Third-party licenses for dictate
================================

dictate is licensed under the GNU General Public License, version 3 or (at
your option) any later version, with an additional permission for NVIDIA CUDA
(see LICENSE and the header of each source file).

This file lists the third-party components that are compiled or embedded into
the binaries dictate, dictate-ui and dictate-settings when they are built from
this repository for x86_64 Linux with the default Cargo features, and
reproduces the license texts and copyright notices those licenses require.

It is generated by scripts/third-party-licenses.py from Cargo.lock and the
published sources of each package; do not edit it by hand.

Not covered here: libraries that the binaries load from the user's system at
run time and that are not distributed with dictate, such as GTK 4,
gtk4-layer-shell, Pango, Cairo, GLib, PulseAudio, libstdc++, the NVIDIA CUDA
libraries and the NVIDIA driver. They are used under their own licenses.

Contents
  1. Rust standard library
  2. whisper.cpp and ggml (bundled in the crate whisper-rs-sys)
  3. EFF Large Wordlist
  4. Rust crates
  5. License texts
"""


def run_metadata():
    out = subprocess.run(
        ["cargo", "metadata", "--offline", "--locked", "--format-version", "1", "--filter-platform", TARGET],
        cwd=ROOT,
        check=True,
        capture_output=True,
        text=True,
    ).stdout
    return json.loads(out)


def linked_packages(meta):
    """Normale Abhängigkeiten vom Wurzelpaket aus (dep_kinds mit kind null), ohne das Wurzelpaket."""
    pkgs = {p["id"]: p for p in meta["packages"]}
    nodes = {n["id"]: n for n in meta["resolve"]["nodes"]}
    root = meta["resolve"]["root"]
    seen, stack = set(), [root]
    while stack:
        i = stack.pop()
        if i in seen:
            continue
        seen.add(i)
        for d in nodes[i]["deps"]:
            if any(k["kind"] is None for k in d["dep_kinds"]):
                stack.append(d["pkg"])
    seen.discard(root)
    return sorted((pkgs[i] for i in seen), key=lambda p: (p["name"], version_key(p["version"])))


def version_key(v):
    return [int(x) if x.isdigit() else x for x in re.split(r"[.+-]", v)]


def normalize(text):
    lines = [line.rstrip() for line in text.replace("\r\n", "\n").replace("\r", "\n").split("\n")]
    while lines and not lines[0]:
        lines.pop(0)
    while lines and not lines[-1]:
        lines.pop()
    return "\n".join(lines) + "\n"


def read_text(path):
    with open(path, "rb") as f:
        return normalize(f.read().decode("utf-8", errors="replace"))


def crate_license_files(crate_dir):
    """Lizenzdateien des Pakets: im Wurzelordner und verschachtelt (ohne SKIP_DIRS), sortiert nach relativem Pfad."""
    found = []
    for cur, dirs, files in os.walk(crate_dir):
        dirs[:] = sorted(d for d in dirs if d not in SKIP_DIRS)
        for f in sorted(files):
            if LICENSE_FILE.match(f):
                found.append(os.path.relpath(os.path.join(cur, f), crate_dir))
    return sorted(found)


def elected(expr):
    """Bei einem reinen OR-Ausdruck die gewählte Lizenz, sonst None (Ausdruck gilt wie angegeben)."""
    e = expr.replace("/", " OR ")
    if "(" in e or " AND " in e:
        return None
    options = [o.strip() for o in e.split(" OR ")]
    if len(options) < 2:
        return None
    for p in PREFERENCE:
        if p in options:
            return p
    return None


def rust_std():
    with open(os.path.join(ROOT, "rust-toolchain.toml"), "rb") as f:
        version = tomllib.load(f)["toolchain"]["channel"]
    sysroot = subprocess.run(["rustc", "--print", "sysroot"], cwd=ROOT, check=True, capture_output=True, text=True).stdout.strip()
    src = os.path.join(sysroot, "share", "doc", "rust", "COPYRIGHT-library.html")
    with open(src, "rb") as f:
        data = f.read()
    return version, f"rust-std-{version}-COPYRIGHT-library.html", data


def whisper_section(pkgs):
    sys_pkg = next((p for p in pkgs if p["name"] == "whisper-rs-sys"), None)
    if sys_pkg is None:
        raise SystemExit("whisper-rs-sys is no longer a dependency: update scripts/third-party-licenses.py")
    base = os.path.join(os.path.dirname(sys_pkg["manifest_path"]), "whisper.cpp")
    cm = open(os.path.join(base, "CMakeLists.txt"), encoding="utf-8").read()
    wv = re.search(r'project\("whisper\.cpp" VERSION ([0-9.]+)\)', cm).group(1)
    gm = open(os.path.join(base, "ggml", "CMakeLists.txt"), encoding="utf-8").read()
    gv = ".".join(re.search(rf"set\(GGML_VERSION_{k} (\d+)\)", gm).group(1) for k in ("MAJOR", "MINOR", "PATCH"))
    # Hinweise fremder Urheber in Dateien, die für x86_64 mit CUDA kompiliert werden (gemessen am Build: ops.cpp.o,
    # rope.cu.o; llamafile/sgemm.cpp und kleidiai werden für dieses Ziel nicht gebaut)
    notices = []
    for rel in ("ggml/src/ggml-cpu/ops.cpp", "ggml/src/ggml-cuda/rope.cu"):
        lines = open(os.path.join(base, rel), encoding="utf-8").read().split("\n")
        for i, line in enumerate(lines):
            if "Copyright" in line and "MIT licensed" in line:
                notices.append((rel, lines[i - 1].strip(), line.strip()))
    if not notices:
        raise SystemExit("YaRN notice not found in ggml: check whisper.cpp for third-party notices again")
    text = [
        f"whisper-rs-sys {sys_pkg['version']} bundles the source code of whisper.cpp {wv}",
        f"and ggml {gv}, which is compiled into the dictate binary (CPU and CUDA",
        "backends). License: MIT; the full text (\"Copyright (c) 2023-2024 The ggml",
        "authors\") is reproduced in section 5 under whisper-rs-sys.",
        "",
        "Two compiled files contain code from a further project:",
        "",
    ]
    for rel, a, b in notices:
        text += [f"  whisper.cpp/{rel}:", f"    {a}", f"    {b}"]
    text += [
        "",
        "License of that project (https://github.com/jquesnelle/yarn, file LICENSE):",
        "",
        read_text(os.path.join(UPSTREAM_DIR, YARN_DIR, "LICENSE")).rstrip("\n"),
    ]
    return "\n".join(text) + "\n"


def build(meta):
    pkgs = linked_packages(meta)
    errors = []
    texts = {}  # Hash -> (Text, Liste "Paket Version: Datei")
    rows = []
    used_upstream = {YARN_DIR}
    for p in pkgs:
        name, ver, lic = p["name"], p["version"], p.get("license") or "(none)"
        crate_dir = os.path.dirname(p["manifest_path"])
        files = [(f, os.path.join(crate_dir, f)) for f in crate_license_files(crate_dir)]
        up = UPSTREAM.get((name, ver))
        if up:
            used_upstream.add(up)
            d = os.path.join(UPSTREAM_DIR, up)
            if not os.path.isdir(d):
                errors.append(f"{name} {ver}: licenses/upstream/{up} is missing")
            else:
                files += [(f"{f} (upstream, see licenses/upstream/SOURCES.md)", os.path.join(d, f)) for f in sorted(os.listdir(d))]
        note = ""
        if name in SAME_AS_DICTATE:
            if lic != SAME_AS_DICTATE[name]:
                errors.append(f"{name} {ver}: license changed to {lic}")
            note = "same license as dictate, full text in LICENSE"
        elif not files:
            errors.append(f"{name} {ver} ({lic}): no license file in the crate and none in licenses/upstream")
        if name in NOTES:
            note = NOTES[name]
        for label, path in files:
            t = read_text(path)
            h = hashlib.sha256(t.encode()).hexdigest()
            texts.setdefault(h, (t, []))[1].append(f"{name} {ver}: {label}")
        el = elected(lic)
        rows.append((name, ver, lic, el, p.get("repository") or "", note))
    stale = sorted(set(os.listdir(UPSTREAM_DIR)) - used_upstream - {"SOURCES.md"}) if os.path.isdir(UPSTREAM_DIR) else []
    for s in stale:
        errors.append(f"licenses/upstream/{s} is not used by any package any more")
    for n in sorted(set(NOTES) - {p["name"] for p in pkgs}):
        errors.append(f"NOTES: {n} is not used by any package any more")
    if errors:
        raise SystemExit("third-party-licenses: " + "\n  ".join(["errors:"] + errors))

    version, html_name, html = rust_std()
    out = [HEADER]
    out.append(section(1, "Rust standard library"))
    out.append(
        f"The Rust standard library of the pinned toolchain {version} is linked\n"
        "statically into every binary. It is dual-licensed under Apache-2.0 or MIT\n"
        "and contains third-party parts under further licenses. Its copyright\n"
        f"notices are in licenses/{html_name}, an unmodified copy\n"
        "of share/doc/rust/COPYRIGHT-library.html from that toolchain.\n"
    )
    out.append(section(2, "whisper.cpp and ggml"))
    out.append(whisper_section(pkgs))
    out.append(section(3, "EFF Large Wordlist"))
    out.append(EFF_SECTION)
    out.append(section(4, "Rust crates"))
    out.append(
        f"{len(rows)} packages. \"Used under\" names the license dictate elects where a\n"
        "package offers a choice; otherwise the package is used under its license\n"
        "expression as stated. Every license file a package ships is reproduced in\n"
        "section 5, also for licenses not elected. Packages under MPL-2.0 are used\n"
        "unmodified; their source code is available from the listed repository and\n"
        "from crates.io.\n\n"
    )
    for name, ver, lic, el, repo, note in rows:
        out.append(f"  {name} {ver}\n    License: {lic}\n")
        if el:
            out.append(f"    Used under: {el}\n")
        if repo:
            out.append(f"    Source: {repo}\n")
        if note:
            out.append("    Note: " + note.replace("\n", "\n          ") + "\n")
    out.append("\n" + section(5, "License texts"))
    out.append(f"{len(texts)} distinct texts. Each text is preceded by the packages and files it comes from.\n")
    for _h, (t, users) in sorted(texts.items(), key=lambda kv: (sorted(kv[1][1])[0], kv[0])):
        out.append("\n" + "-" * 79 + "\n")
        out.append("Used by:\n" + "".join(f"  {u}\n" for u in sorted(users)))
        out.append("-" * 79 + "\n\n")
        out.append(t)
    return "".join(out), html_name, html


def section(n, title):
    line = f"{n}. {title}"
    return f"\n{line}\n{'-' * len(line)}\n\n"


def main():
    check = sys.argv[1:] == ["--check"]
    if sys.argv[1:] and not check:
        raise SystemExit("usage: scripts/third-party-licenses.py [--check]")
    text, html_name, html = build(run_metadata())
    html_path = os.path.join(LIC_DIR, html_name)
    old_html = [f for f in os.listdir(LIC_DIR) if f.startswith("rust-std-") and f != html_name]
    if check:
        bad = []
        if not os.path.exists(OUT) or open(OUT, encoding="utf-8").read() != text:
            bad.append("THIRD-PARTY-LICENSES.txt")
        if not os.path.exists(html_path) or open(html_path, "rb").read() != html:
            bad.append(f"licenses/{html_name}")
        bad += [f"licenses/{f} (old toolchain)" for f in old_html]
        if bad:
            print("FAILED: out of date: " + ", ".join(bad) + "; run scripts/third-party-licenses.py and commit the result")
            return 1
        print(f"licenses ok ({text.count(chr(10))} lines)")
        return 0
    with open(OUT, "w", encoding="utf-8") as f:
        f.write(text)
    with open(html_path, "wb") as f:
        f.write(html)
    for f in old_html:
        os.remove(os.path.join(LIC_DIR, f))
    print(f"wrote THIRD-PARTY-LICENSES.txt and licenses/{html_name}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
