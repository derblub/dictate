#!/usr/bin/env python3
# Copyright (C) 2026 Daniel Kurdoghlian
# Pushing Pixels, Vienna <https://pushingpixels.at>
# SPDX-License-Identifier: GPL-3.0-or-later
"""Zielfenster für den Prüfschritt `wayland`: GTK-4-Fenster (app_id `dictate-test-target`), das Fokus und Klicks als
Zeilen auf stdout meldet (`focus in`, `focus out`, `click X Y`). Läuft nur im verschachtelten niri des Tests.

`target.py text APP_ID [TEXT]`: Fenster mit einem mehrzeiligen Textfeld (GTK 4 spricht text-input-v3, also
Eingabemethode T3), meldet jeden neuen Inhalt als `text <JSON>` (nur Testtexte) und Tasten mit Modifikatoren als
`key NAME MODS`. Mit TEXT steht der Text im Feld und ist ganz markiert (PRIMARY, Textaktionen W3). Der Titel ist die
APP_ID, außer die Umgebung setzt `TARGET_TITLE` (Testtext der Palette im Kontext, W5)."""
import json
import os
import sys

import gi

gi.require_version("Gtk", "4.0")
from gi.repository import GLib, Gtk  # noqa: E402


def say(line):
    print(line, flush=True)


def text_window(app_id, initial=None):
    GLib.set_prgname(app_id)
    Gtk.init()
    win = Gtk.Window(title=os.environ.get("TARGET_TITLE") or app_id)
    win.set_default_size(800, 600)
    view = Gtk.TextView()
    buf = view.get_buffer()
    buf.connect("changed", lambda b: say("text " + json.dumps(b.get_text(b.get_start_iter(), b.get_end_iter(), False))))
    keys = Gtk.EventControllerKey()
    keys.set_propagation_phase(Gtk.PropagationPhase.CAPTURE)

    def on_key(_c, keyval, _code, state):
        mods = int(state & (Gtk.accelerator_get_default_mod_mask()))
        if mods:
            say(f"key {Gtk.accelerator_name(keyval, 0)} {mods}")
        return False

    keys.connect("key-pressed", on_key)
    view.add_controller(keys)
    win.set_child(view)
    win.connect("close-request", lambda _w: (loop.quit(), False)[1])
    win.present()
    view.grab_focus()

    def select_all():
        buf.select_range(buf.get_start_iter(), buf.get_end_iter())
        say("selected")
        return False

    if initial is not None:
        buf.set_text(initial)
        # erst mit Fokus markieren: der Compositor nimmt PRIMARY nur mit der Seriennummer einer Eingabe an
        state = {"done": False}

        def on_active(w, _p):
            if w.is_active() and not state["done"]:
                state["done"] = True
                GLib.timeout_add(150, select_all)

        win.connect("notify::is-active", on_active)

    loop = GLib.MainLoop()
    loop.run()


def main():
    if len(sys.argv) > 2 and sys.argv[1] == "text":
        return text_window(sys.argv[2], sys.argv[3] if len(sys.argv) > 3 else None)
    GLib.set_prgname("dictate-test-target")
    Gtk.init()
    win = Gtk.Window(title="dictate test target")
    win.set_default_size(800, 600)
    click = Gtk.GestureClick()
    click.set_button(0)
    click.connect("pressed", lambda _g, _n, x, y: say(f"click {round(x)} {round(y)}"))
    win.add_controller(click)
    focus = Gtk.EventControllerFocus()
    focus.connect("enter", lambda _c: say("focus in"))
    focus.connect("leave", lambda _c: say("focus out"))
    win.add_controller(focus)
    win.connect("notify::is-active", lambda w, _p: say(f"active {w.is_active()}"))
    win.connect("close-request", lambda _w: (loop.quit(), False)[1])
    win.present()
    loop = GLib.MainLoop()
    loop.run()


if __name__ == "__main__":
    sys.exit(main())
