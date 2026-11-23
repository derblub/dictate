# Lizenzdateien aus den Upstream-Repositories

Für Pakete, deren veröffentlichtes Crate keine Lizenzdatei enthält (Feld `license` in `Cargo.toml` ohne Datei), und
für den YaRN-Hinweis in ggml. Unverändert geladen am 23.11.2026; Commit aus `.cargo_vcs_info.json` des Crates
(der Stand, aus dem es veröffentlicht wurde). `scripts/third-party-licenses.py` ordnet die Ordner den Paketen zu
(`UPSTREAM`) und bricht ab, wenn sich eine Version ändert oder ein Ordner nicht mehr gebraucht wird.

| Ordner | Pakete | Quelle |
|---|---|---|
| `accesskit-0.25.1` | accesskit 0.25.1 | github.com/AccessKit/accesskit, Commit `ce8164ba92995cfa86005b6259115e08c8244253` (Tag `accesskit-v0.25.1`): `LICENSE-APACHE`, `LICENSE-MIT`, `LICENSE.chromium` |
| `egui-0.37.0` | ecolor, eframe, egui, egui-wgpu, egui-winit, egui_glow, emath, epaint 0.37.0 | github.com/emilk/egui, Tag `0.37.0` (Commit `841a328ba2dd8f6996beb7189ebb7fc00e668a2a`): `LICENSE-APACHE`, `LICENSE-MIT`. Der in den Crates genannte Commit `7c0b40402271…` liegt nicht auf GitHub (404); eframe nennt `3c6eb8261c32…`, dort sind beide Dateien byte-gleich mit dem Tag |
| `gl-0.14.0` | gl 0.14.0 | github.com/brendanzab/gl-rs, Commit `ea503e8d5fb6d73c6030e6191ce738cd3bf3433e` (Tag `gl-v0.14.0`): `LICENSE` |
| `gtk4-layer-shell-0.8.1` | gtk4-layer-shell 0.8.1 | github.com/pentamassiv/gtk4-layer-shell-gir, Commit `7043a44adc4e31868e6f30a4f8c8279168417a94`: `LICENSE` |
| `gtk4-layer-shell-sys-0.6.1` | gtk4-layer-shell-sys 0.6.1 | github.com/pentamassiv/gtk4-layer-shell-gir, Commit `c52b9b12e6b1afa38dcb496ec821c55bb298da78`: `LICENSE` |
| `profiling-1.0.18` | profiling 1.0.18 | github.com/aclysma/profiling, Commit `8271551172eb6fa4cba47369aedd93790c623df9`: `LICENSE-APACHE`, `LICENSE-MIT` |
| `whisper-rs-sys-0.15.0` | whisper-rs-sys 0.15.0 (Unlicense; der gebündelte whisper.cpp bringt seine `LICENSE` selbst mit) | codeberg.org/tazz4843/whisper-rs, Commit `7558e1b72f54f2f22a53589afb77e65681834c36`: `LICENSE` |
| `yarn` | YaRN-Code in `ggml/src/ggml-cpu/ops.cpp` und `ggml/src/ggml-cuda/rope.cu` (Hinweis „MIT licensed. Copyright (c) 2023 Jeffrey Quesnelle and Bowen Peng.“) | github.com/jquesnelle/yarn, Commit `995db5b575e75230b3384d658f8b944c9662f775` (HEAD am 23.11.2026): `LICENSE` |

niri-ipc (GPL-3.0-or-later) hat ebenfalls keine Datei im Crate; sein Text ist der von `LICENSE` (Liste
`SAME_AS_DICTATE` im Skript).
