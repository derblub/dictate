# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses [Semantic Versioning](https://semver.org/).

## [Unreleased]

## [0.1.0] - 2026-11-24

The first public release of dictate. The history before it stays private; the repository starts with a single commit.

### Added

- Live dictation for the niri compositor: hold `Mod+E` and speak, or toggle hands-free mode with `Mod+Shift+E`. An
  overlay shows the words while you speak, and the final text lands in the focused field through its input method,
  the clipboard (restored afterwards, never in terminals) or a virtual keyboard.
- Local speech recognition with whisper.cpp (`large-v3-turbo`, quantized) on an NVIDIA GPU, Silero VAD against
  invented words in pauses, and a boost for quiet microphones.
- Optional AI post-processing with your own key (Anthropic, OpenAI-compatible servers, local servers such as Ollama):
  clean-up, e-mail, translation and turning a spoken request into a structured prompt.
- Text actions on the selection (`Mod+I`, off by default): explain, analyze an error, explain code, translate, rephrase
  and refine a prompt, with a preview of what is sent and a secrets filter before any cloud request.
- Command palette (`Mod+Alt+Space`): windows, workspaces, programs, projects, media, volume, screenshots and lock,
  typed or spoken. Matching is local; the AI only picks an intent from a fixed catalog.
- Linux diagnosis (`Mod+Shift+I`, off by default): read-only probes from a fixed catalog, redaction and pseudonyms,
  the exact report as a preview, then causes and further read-only checks suggested by an AI.
- Settings window `dictate-settings` with model downloads, colors from the Noctalia palette and the system font.
- License GPL-3.0-or-later with an additional permission for NVIDIA CUDA, third-party notices in
  `THIRD-PARTY-LICENSES.txt`, a contributor license agreement (`CLA.md`), a security policy and a code of conduct.
- CI on GitHub: formatting, license checks, clippy and the mock tests on every push to `main` and every pull request.
- Package for Arch Linux in the AUR (`dictate`), built from the release tag with CUDA from the system; the PKGBUILD is
  in `packaging/aur/`.

[Unreleased]: https://github.com/derblub/dictate/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/derblub/dictate/releases/tag/v0.1.0
