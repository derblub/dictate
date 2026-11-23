# Contributing to dictate

Thank you for your interest in dictate. Bug reports, ideas and questions are welcome in the
[issue tracker](https://github.com/derblub/dictate/issues) at any time; please report security problems privately as
described in [SECURITY.md](SECURITY.md), not in a public issue. Everyone taking part follows the
[Code of Conduct](CODE_OF_CONDUCT.md).
Code and other material that should become part of dictate needs a signed contributor license agreement (CLA) first;
this page explains why, how to agree, and how to prepare a change.

**Note:** I adapted the CLA (`CLA.md`) from a widely used template, and it has not been reviewed by a lawyer.

## Contributor License Agreement

dictate is licensed under GPL-3.0-or-later (see `LICENSE` and the README). Before a contribution can be merged, its
author agrees to the dictate Individual Contributor License Agreement in `CLA.md`. In plain words (only `CLA.md` is
binding):

- **You keep the copyright** in your contribution and may use it in any way you like.
- **You grant me a broad license** to use, change and distribute your contribution as part of dictate,
  including the right to license it under other terms, for example in paid builds or under a different license.
- **Your contribution stays free:** whenever I use that right, your contribution is also licensed under
  the license(s) dictate used on the day you submitted it (GPL-3.0-or-later today).
- **You grant a patent license** for your contribution as far as you hold relevant patents.
- **You confirm** that the contribution is yours to give and, if you are employed, that your employer has approved the
  agreement.
- Austrian law applies.

Why a CLA at all: I want to be able to offer paid, supported builds later and to adjust the license without asking
every past contributor again. The promise in the third point makes sure that your work remains available
under the free license either way.

**How to agree:** in your first pull or merge request, add a commit authored by you that adds one line for yourself to
the table in `CLA-SIGNATURES.md` (name, account, date, CLA version), with the commit message given there. Merging that
commit is my acceptance. You do this once; it covers your later contributions to the same CLA version.

## Before you start

- For anything larger than a small fix, open an issue first and describe what you want to change. dictate has a
  narrow scope (see the README) and some design decisions are deliberate; an issue saves you work on a change that
  would not be merged.
- Keep a change focused on one thing. Unrelated clean-ups belong in a separate request.

## Development setup

Building dictate needs the same as running it (README, "Requirements"): Linux, an NVIDIA GPU with the CUDA toolkit,
PulseAudio or PipeWire, GTK 4 and gtk4-layer-shell. The Rust toolchain is pinned in `rust-toolchain.toml` and installed
by rustup automatically. The first CUDA build takes several minutes; avoid `cargo clean`, which throws that work away.

## Checks

`scripts/check.sh` is the single source for all checks: for me, for automated tools and for the CI on GitHub, which
runs `fmt`, `licenses`, `clippy` and `unit` on every pull request (`.github/workflows/ci.yml`). It runs the steps
cheapest first and stops at the first failure:

```bash
scripts/check.sh --list                    # fmt licenses clippy unit wayland gui speech gpu
scripts/check.sh --only fmt,clippy,unit    # only the steps your change touches
```

| Step | What it checks | Needs |
|---|---|---|
| `fmt` | `cargo fmt --check` with `rustfmt.toml` | Rust |
| `licenses` | `cargo deny` against the allow list in `deny.toml`, and that `THIRD-PARTY-LICENSES.txt` matches `Cargo.lock` | `cargo-deny`, Python 3 |
| `clippy` | `cargo clippy --release --all-targets`, warnings are errors | Rust, CUDA toolkit |
| `unit` | `cargo test --release` (mock tests, without the tests marked `ignored`) | Rust, CUDA toolkit |
| `wayland` | `dictate-ui` in a nested niri against a mock daemon (overlay, text output, panels, palette, lock) | niri, sway, grim, xkbcli, Python 3 with Pillow and PyGObject (GTK 4) |
| `gui` | the settings window in a nested niri against a mock daemon | as `wayland` |
| `speech` | a read-aloud test set through a virtual Pulse source with the real model | recordings in `$DICTATE_TEST_ROOT/audio/testset` (skipped without them), GPU |
| `gpu` | the remaining tests with the real model and a Pulse source | GPU, PulseAudio tools, sox, test audio in `$DICTATE_TEST_ROOT/audio` (fails without it) |

**Test folder:** logs, the lock of `check.sh`, the runtime folders of the nested niri and the test audio live in one
folder, set with `DICTATE_TEST_ROOT`. The default is `${XDG_CACHE_HOME:-$HOME/.cache}/dictate-test`; `check.sh`
creates it and prints it at the start, the logs of each run are in `$DICTATE_TEST_ROOT/logs/check/latest/`. The path
must be absolute and may only contain `A-Z a-z 0-9 . _ - /`, and it must be at most 55 characters long, because the
Unix sockets of the nested session are created below it. The lock lives in that folder too, so runs with different
folders do not block each other.

```bash
cargo fetch --locked                       # the license checks run offline and read the fetched crate sources
scripts/check.sh --only fmt,licenses,clippy,unit,wayland,gui
DICTATE_TEST_ROOT=/path/to/test-folder scripts/check.sh --only gui   # another folder
```

`speech` and `gpu` need recordings that are not in the repository: the read-aloud set of `tests/speech/testset.tsv`
in `$DICTATE_TEST_ROOT/audio/testset/<id>.wav` (record your own with `scripts/record-testset.sh`; the thresholds in
`tests/speech/thresholds.tsv` were derived from my voice and microphone), and my recordings
`de05.wav`, `de10.wav`, `de30.wav` and `live-brio.wav` (16 kHz mono) in `$DICTATE_TEST_ROOT/audio` for `gpu`; the
tests check for specific words of these recordings, so other audio fails. Without them, leave these two steps to me.

Say in your request which checks you ran. I run `wayland`, `gui`, `speech` and `gpu` before merging.
Tests that drive a compositor run only in the nested niri of `tests/wayland/nested/harness.sh`, never against your
own session.

## Style and rules

- **Formatting:** `rustfmt.toml` (line width 160); wrap comments by hand at about 120 characters.
- **Lints:** clippy warnings are errors.
- **Tests:** new behavior comes with a test; a bug fix with a test that failed before the fix.
- **Privacy:** never log what the user says or selects, clipboard contents, AI answers, diagnosis reports or probe
  output, not even in debug logs. Existing tests check parts of this.
- **No shell:** programs are started with an argument vector, never through `sh -c` or similar; tests in
  `src/bin/dictate-ui/` enforce this.
- **Dependencies:** a new or updated crate changes `Cargo.lock`; then run `python3 scripts/third-party-licenses.py` and
  commit the regenerated `THIRD-PARTY-LICENSES.txt`. Only licenses compatible with GPL-3.0-or-later can be added to
  `deny.toml`. dictate bundles no fonts.

## License headers

Every new `.rs` file starts with the license header, byte for byte as `RS_HEADER` in `tests/license_headers.rs`,
followed by an empty line. Every new shell or Python script carries the short header `SCRIPT_HEADER` from the same file
directly after the shebang line. Copy them from there; the test in the `unit` step finds missing or altered headers.

The header names the copyright holder of dictate. It does not take away your copyright in your contribution: that stays
with you under the CLA, and the Git history records who wrote what.

## AI-assisted contributions

You may use AI tools to write code or text. If you do, say so in your pull or merge request: which tool, and which
parts it produced or changed substantially. You must understand and have reviewed everything you submit, and you remain
responsible for the confirmations in the CLA, in particular that the contribution contains no material you have no
right to give (`CLA.md`, Section 3 and Annex A).

## Third-party material

Do not copy code, text, images, data or fonts from other projects without asking in an issue first. If such material is
accepted, mark it in your request with its source, author and license, and keep its notices; its license must be
compatible with GPL-3.0-or-later (details in `CLA.md`, Annex A). Prefer material under a permissive license such as
MIT, BSD or Apache-2.0: copyleft material cannot be offered under other terms as the CLA allows for your own work, so
it needs my explicit agreement.

## Language

User-facing texts in the program, the README and this file are in English. Internal plans and most code comments are
in German. For your own comments, commit messages, issues and requests, English is fine.

## Pull and merge requests

Please include:

- what the change does and why (link the issue);
- which checks you ran;
- whether and how you used AI tools;
- for your first contribution, the commit that adds you to `CLA-SIGNATURES.md`.
