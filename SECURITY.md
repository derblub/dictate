# Security policy

## Supported versions

Security fixes go into the latest commit on `main` until there are releases, then into the latest release. Please
update before reporting.

## Reporting a vulnerability

Please do not open a public issue. Report it privately through
[GitHub's vulnerability reporting](https://github.com/derblub/dictate/security/advisories/new) or by email to
[daniel@pushingpixels.at](mailto:daniel@pushingpixels.at).

Helpful details: the version or commit, what an attacker can do, the steps to reproduce it, and whether the problem
needs a local user, another program on the machine or only network access.

You will get an answer within five working days. Once a fix is released, the report is published as a security
advisory and credited in the changelog unless you prefer otherwise.

## Scope

In scope is the code in this repository: the daemon `dictate`, the Wayland front end `dictate-ui`, the settings window
`dictate-settings`, their protocol and socket, the secrets filter used before text is sent to an AI service, the
read-only probes of the Linux diagnosis, and the scripts and units shipped here.

Out of scope: problems in the compositor, the GPU driver, CUDA, whisper.cpp or another dependency (please report those
upstream, and tell me too if dictate needs to react), and anything that already requires control of your user account.

## What dictate does with your data

Speech recognition runs on your own GPU, and no audio leaves the machine. The default AI server is a local Ollama:
text leaves the machine only if you choose a cloud provider, with your own key. The section
[Privacy and security](README.md#privacy-and-security) in the README describes what is sent, what is filtered and
what is never logged.
