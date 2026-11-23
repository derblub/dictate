# Copyright (C) 2026 Daniel Kurdoghlian
# Pushing Pixels, Vienna <https://pushingpixels.at>
# SPDX-License-Identifier: GPL-3.0-or-later
# Testordner von dictate (PLAN-lizenz.md, O7 und G3): Logs der Prüfläufe, Testaudio, der Laufzeitordner des
# verschachtelten niri (wl-run/) und die Sperre von scripts/check.sh liegen unter einem Ordner, eingestellt über
# DICTATE_TEST_ROOT. Vorgabe ${XDG_CACHE_HOME:-$HOME/.cache}/dictate-test; ein relatives XDG_CACHE_HOME zählt nach der
# XDG-Spezifikation nicht. Auf blub zeigt die Variable auf /home/data/stt-test (CLAUDE.md, „Bauen und installieren“).
# Zwei Ordner heißen zwei Sperren: zwei Läufe von check.sh mit verschiedenen Ordnern sperren sich nicht gegenseitig.
#
# Eingebunden (source) von scripts/check.sh, scripts/record-testset.sh und tests/wayland/nested/harness.sh. Dieselbe
# Regel gilt in Rust: tests/test_root/mod.rs (Integrationstests, live_eval) und tests/wayland/nested/src/guard.rs
# (Wächter des Injektors und der Zwischenablage, dort ohne Vorgabe).
#
# dictate_test_root: löst den Ordner auf, prüft ihn und exportiert DICTATE_TEST_ROOT (ohne Schrägstrich am Ende); bei
# einem untauglichen Ordner Meldung auf stderr und Rückgabe 1. Erlaubt sind nur absolute Pfade ohne leere, `.`- und
# `..`-Teile aus A-Z a-z 0-9 . _ - / (der Pfad landet in `rm -rf`, in Globs von compgen und in der Adresse des
# privaten D-Bus, die andere Zeichen maskiert). Legt nichts an.

dictate_test_root() {
    local root="${DICTATE_TEST_ROOT:-}" cache="${XDG_CACHE_HOME:-}"
    local LC_ALL=C # Bereiche wie A-Z im regulären Ausdruck byteweise, unabhängig von der Locale
    if [[ -z $root ]]; then
        if [[ $cache != /* ]]; then
            [[ ${HOME:-} == /* ]] || {
                echo "DICTATE_TEST_ROOT is not set and HOME is not an absolute path; set DICTATE_TEST_ROOT." >&2
                return 1
            }
            cache="$HOME/.cache"
        fi
        root="$cache/dictate-test"
    fi
    while [[ $root == */ && $root != / ]]; do root="${root%/}"; done
    if [[ $root != /* || $root == / ]]; then
        echo "DICTATE_TEST_ROOT must be an absolute path below / (got '$root')." >&2
        return 1
    fi
    if [[ ! $root =~ ^[A-Za-z0-9._/-]+$ ]]; then
        echo "DICTATE_TEST_ROOT may only contain A-Z a-z 0-9 . _ - / (got '$root'); set it to such a folder." >&2
        return 1
    fi
    if [[ $root/ == *//* || $root/ == */./* || $root/ == */../* ]]; then
        echo "DICTATE_TEST_ROOT must not contain empty, '.' or '..' parts (got '$root')." >&2
        return 1
    fi
    export DICTATE_TEST_ROOT="$root"
}
