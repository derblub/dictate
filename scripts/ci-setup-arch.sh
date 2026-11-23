#!/usr/bin/env bash
# Copyright (C) 2026 Daniel Kurdoghlian
# Pushing Pixels, Vienna <https://pushingpixels.at>
# SPDX-License-Identifier: GPL-3.0-or-later
# Bereitet einen frischen Container `archlinux:latest` für die Schritte fmt, licenses, clippy und unit von
# scripts/check.sh vor (CI auf GitHub, .github/workflows/ci.yml; lokal nachstellbar mit docker run, siehe
# PLAN-lizenz.md U5c). Arch statt Ubuntu, weil dictate-ui GTK 4.24 und gtk4-layer-shell 1.3 braucht (Entscheidung
# Daniel, 23.11.2026). Läuft als root im Container, nie auf dem Rechner des Autors.
#
# Pakete: Bauwerkzeuge, CUDA-Toolkit, GTK 4 mit gtk4-layer-shell, PulseAudio-Bibliothek, Python für
# scripts/third-party-licenses.py, cargo-deny für den Schritt licenses. Die Toolchain kommt aus rust-toolchain.toml, die
# Quellen der Crates holt `cargo fetch` (licenses läuft offline).
#
# libcuda.so.1 (NEEDED von allem, was die Engine linkt) gehört zum Treiber (nvidia-utils, 1 GB), nicht zum Toolkit. Ohne
# GPU genügt der Stub des Toolkits (`lib64/stubs/libcuda.so`, SONAME libcuda.so.1): er lädt, jeder Aufruf meldet
# CUDA_ERROR_STUB_LIBRARY, und kein Test ohne `--ignored` ruft CUDA (U5c: alle Test-Binaries im Container mit dem Stub
# grün). Eingebunden über ld.so.conf.d, nur im Container.
set -euo pipefail

[[ -f /etc/arch-release ]] || { echo "FAILED: not an Arch Linux container" >&2; exit 1; }
[[ $(id -u) -eq 0 ]] || { echo "FAILED: needs root (container only)" >&2; exit 1; }
cd "$(dirname "$0")/.."

pacman -Syu --noconfirm --needed \
  base-devel git rustup cmake clang \
  cuda gtk4 gtk4-layer-shell libpulse python cargo-deny
# Paketcache frei machen: auf dem Runner ist der Platz knapp (CUDA allein über 5 GiB)
rm -rf /var/cache/pacman/pkg/*

mkdir -p /opt/cuda-stub
ln -sf /opt/cuda/lib64/stubs/libcuda.so /opt/cuda-stub/libcuda.so.1
echo /opt/cuda-stub >/etc/ld.so.conf.d/cuda-stub.conf
ldconfig

rustup toolchain install # Fassung und Komponenten aus rust-toolchain.toml
cargo fetch --locked
