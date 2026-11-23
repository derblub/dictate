// Copyright (C) 2026 Daniel Kurdoghlian
// Pushing Pixels, Vienna <https://pushingpixels.at>
//
// This program is free software; you can redistribute it and/or modify it
// under the terms of the GNU General Public License as published by the Free
// Software Foundation; either version 3 of the License, or (at your option)
// any later version.
//
// This program is distributed in the hope that it will be useful, but
// WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY
// or FITNESS FOR A PARTICULAR PURPOSE.  See the GNU General Public License
// for more details.
//
// You should have received a copy of the GNU General Public License along
// with this program; if not, see <https://www.gnu.org/licenses>.
//
// Additional permission under GNU GPL version 3 section 7
//
// If you modify this Program, or any covered work, by linking or combining
// it with NVIDIA CUDA (the libraries of the NVIDIA CUDA Toolkit, such as
// libcudart, libcublas and libcublasLt, and the CUDA driver library libcuda
// of the NVIDIA driver) (or a modified version of that library), containing
// parts covered by the terms of the NVIDIA CUDA Toolkit End User License
// Agreement or the NVIDIA Driver License Agreement, the licensors of this
// Program grant you additional permission to convey the resulting work.

//! Spike S4 (W0, PLAN-wayland.md Abschnitt 5): GTK 4 unter niri.
//!
//! `wl-gtk-probe overlay [--region empty|full] [--keyboard none|exclusive|on-demand]`
//!     Layer-Fläche auf `Overlay`, Namensraum `dictate-overlay`, unten mittig 640×120 logisch. `--region empty` setzt
//!     nach jedem Map eine leere Eingaberegion (klick-durch).
//! `wl-gtk-probe panel [--app-id ID]`
//!     Langlebiger Prozess ohne Fenster; `show` legt das Panel (xdg_toplevel, app_id `dictate-panel`, Eingabefeld) an
//!     bzw. zeigt es wieder, `hide` blendet aus (unmap), `close` zerstört es, `size W H` ändert die Größe.
//! `wl-gtk-probe target NAME`
//!     Zielfenster (app_id `probe-target`, Titel NAME) mit Eingabefeld; meldet jede Textänderung.
//!
//! Befehle über stdin (eine Zeile je Befehl), Beobachtungen auf stdout als eine Zeile:
//! `ready`, `mapped W H`, `scale S`, `active true|false`, `focus in|out`, `key press|release NAME CODE`,
//! `click X Y`, `text "…"`, `region empty|full`. Das Testskript wertet diese Zeilen aus.
//!
//! Isolation: startet nur, wenn XDG_RUNTIME_DIR unter /home/data/stt-test/wl-run/ liegt (Harness).

use std::cell::RefCell;
use std::io::Write;
use std::rc::Rc;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use gtk::prelude::*;
use gtk::{cairo, gdk, glib};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};

thread_local! {
    static START: Instant = Instant::now();
}

fn say(line: &str) {
    let ms = START.with(|s| s.elapsed().as_millis());
    let mut out = std::io::stdout();
    let _ = writeln!(out, "{ms:>7} {line}").and_then(|()| out.flush());
}

fn guard() {
    const BASE: &str = "/home/data/stt-test/wl-run/";
    let rt = std::env::var("XDG_RUNTIME_DIR").unwrap_or_default();
    let wl = std::env::var("WAYLAND_DISPLAY").unwrap_or_default();
    if !rt.starts_with(BASE) || (wl.starts_with('/') && !wl.starts_with(BASE)) {
        eprintln!("wl-gtk-probe: refusing to run outside the test runtime dir {BASE}");
        std::process::exit(70);
    }
}

fn add_logging(window: &gtk::Window) {
    let click = gtk::GestureClick::new();
    click.set_button(0);
    click.connect_pressed(|_, _, x, y| say(&format!("click {} {}", x.round(), y.round())));
    window.add_controller(click);
    let keys = gtk::EventControllerKey::new();
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    keys.connect_key_pressed(|_, keyval, code, _| {
        say(&format!("key press {} {code}", keyval.name().map(|n| n.to_string()).unwrap_or_default()));
        glib::Propagation::Proceed
    });
    keys.connect_key_released(|_, keyval, code, _| {
        say(&format!("key release {} {code}", keyval.name().map(|n| n.to_string()).unwrap_or_default()));
    });
    keys.connect_modifiers(|_, state| {
        say(&format!("modifiers {state:?}"));
        glib::Propagation::Proceed
    });
    window.add_controller(keys);
    let focus = gtk::EventControllerFocus::new();
    focus.connect_enter(|_| say("focus in"));
    focus.connect_leave(|_| say("focus out"));
    window.add_controller(focus);
    window.connect_is_active_notify(|w| say(&format!("active {}", w.is_active())));
    window.connect_map(|w| {
        let w = w.clone();
        glib::timeout_add_local_once(Duration::from_millis(150), move || {
            say(&format!("mapped {} {}", w.width(), w.height()));
            if let Some(s) = w.surface() {
                say(&format!("scale {} (int {})", s.scale(), s.scale_factor()));
                s.connect_scale_notify(|s| say(&format!("scale {} (int {})", s.scale(), s.scale_factor())));
            }
        });
    });
}

fn entry_logging(entry: &gtk::Entry) {
    entry.connect_changed(|e| say(&format!("text {:?}", e.text().as_str())));
}

fn css() {
    let css = gtk::CssProvider::new();
    css.load_from_string(
        "window.overlay { background: transparent; } \
         .card { background: #79b8cc; color: #0b1418; padding: 18px; font-size: 22px; } \
         window.panel { background: #1d2326; color: #e8ecee; } \
         window.target { background: #464442; color: #ffffff; } \
         entry { font-size: 20px; }",
    );
    gtk::style_context_add_provider_for_display(
        &gdk::Display::default().expect("display"),
        &css,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}

fn set_region(window: &gtk::Window, region: &str) {
    let Some(surface) = window.surface() else { return };
    match region {
        "empty" => surface.set_input_region(Some(&cairo::Region::create())),
        "full" => {
            let r = cairo::RectangleInt::new(0, 0, window.width().max(1), window.height().max(1));
            surface.set_input_region(Some(&cairo::Region::create_rectangle(&r)));
        }
        _ => return,
    }
    say(&format!("region {region}"));
}

fn keyboard_mode(name: &str) -> KeyboardMode {
    match name {
        "exclusive" => KeyboardMode::Exclusive,
        "on-demand" => KeyboardMode::OnDemand,
        _ => KeyboardMode::None,
    }
}

fn build_overlay(args: &[String]) -> gtk::Window {
    let opt = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    let region = opt("--region").unwrap_or_else(|| "empty".into());
    let window = gtk::Window::new();
    window.add_css_class("overlay");
    add_logging(&window);
    window.init_layer_shell();
    window.set_layer(Layer::Overlay);
    window.set_namespace(Some("dictate-overlay"));
    window.set_exclusive_zone(-1);
    window.set_anchor(Edge::Bottom, true);
    window.set_margin(Edge::Bottom, 80);
    window.set_keyboard_mode(keyboard_mode(&opt("--keyboard").unwrap_or_else(|| "none".into())));
    let label = gtk::Label::new(Some("Listening … Grüße, ß"));
    label.add_css_class("card");
    label.set_size_request(640, 120);
    window.set_child(Some(&label));
    window.connect_map(move |w| {
        let (w, region) = (w.clone(), region.clone());
        glib::timeout_add_local_once(Duration::from_millis(100), move || set_region(&w, &region));
    });
    say(&format!("layer-shell {}", window.is_layer_window()));
    window
}

fn build_panel(app_id: &str) -> gtk::Window {
    glib::set_prgname(Some(app_id));
    let window = gtk::Window::new();
    window.add_css_class("panel");
    window.set_title(Some("dictate panel"));
    window.set_default_size(720, 420);
    add_logging(&window);
    let vbox = gtk::Box::new(gtk::Orientation::Vertical, 12);
    vbox.set_margin_top(18);
    vbox.set_margin_start(18);
    vbox.set_margin_end(18);
    let label = gtk::Label::new(Some("Ask about the selection – Grüße, Maß"));
    let entry = gtk::Entry::new();
    entry_logging(&entry);
    vbox.append(&label);
    vbox.append(&entry);
    window.set_child(Some(&vbox));
    entry.grab_focus();
    window
}

fn build_target(name: &str) -> gtk::Window {
    let window = gtk::Window::new();
    window.add_css_class("target");
    window.set_title(Some(name));
    window.set_default_size(900, 600);
    add_logging(&window);
    let entry = gtk::Entry::new();
    entry.set_valign(gtk::Align::Start);
    entry.set_margin_top(30);
    entry.set_margin_start(30);
    entry.set_margin_end(30);
    entry_logging(&entry);
    window.set_child(Some(&entry));
    window
}

fn main() {
    guard();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mode = args.first().cloned().unwrap_or_default();
    let opt = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    match mode.as_str() {
        "target" => glib::set_prgname(Some("probe-target")),
        "overlay" => glib::set_prgname(Some("dictate-overlay")),
        "panel" => {}
        _ => {
            eprintln!("usage: wl-gtk-probe overlay|panel|target …");
            std::process::exit(2);
        }
    }
    gtk::init().expect("gtk init");
    css();
    START.with(|_| ());

    let window: Rc<RefCell<Option<gtk::Window>>> = Rc::new(RefCell::new(None));
    let app_id = opt("--app-id").unwrap_or_else(|| "dictate-panel".into());
    match mode.as_str() {
        "overlay" => {
            let w = build_overlay(&args);
            w.present();
            *window.borrow_mut() = Some(w);
        }
        "target" => {
            let w = build_target(args.get(1).map(String::as_str).unwrap_or("target"));
            w.present();
            *window.borrow_mut() = Some(w);
        }
        _ => {} // panel: erst auf `show`
    }

    let (tx, rx) = mpsc::channel::<String>();
    std::thread::spawn(move || {
        for line in std::io::stdin().lines().map_while(Result::ok) {
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    let main_loop = glib::MainLoop::new(None, false);
    {
        let ml = main_loop.clone();
        glib::timeout_add_local(Duration::from_millis(5), move || {
            loop {
                let line = match rx.try_recv() {
                    Ok(line) => line,
                    Err(mpsc::TryRecvError::Empty) => return glib::ControlFlow::Continue,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        ml.quit();
                        return glib::ControlFlow::Break;
                    }
                };
                say(&format!("cmd {line}"));
                let parts: Vec<&str> = line.split_whitespace().collect();
                let current = window.borrow().clone();
                match (parts.as_slice(), current) {
                    (["show"], None) if mode == "panel" => {
                        let w = build_panel(&app_id);
                        w.present();
                        *window.borrow_mut() = Some(w);
                    }
                    (["show"], Some(w)) => w.present(),
                    (["hide"], Some(w)) => w.set_visible(false),
                    (["close"], Some(w)) => {
                        w.destroy();
                        *window.borrow_mut() = None;
                    }
                    (["size", ww, hh], Some(w)) => {
                        if let (Ok(ww), Ok(hh)) = (ww.parse(), hh.parse()) {
                            w.set_default_size(ww, hh);
                        }
                    }
                    // Inhalt wächst (wie gestreamte Antwort): Mindestgröße des Kindes, das Fenster folgt.
                    (["grow", ww, hh], Some(w)) => {
                        if let (Ok(ww), Ok(hh), Some(c)) = (ww.parse(), hh.parse(), w.child()) {
                            c.set_size_request(ww, hh);
                        }
                    }
                    (["keyboard", m], Some(w)) if w.is_layer_window() => w.set_keyboard_mode(keyboard_mode(m)),
                    (["region", r], Some(w)) => set_region(&w, r),
                    (["quit"], _) => {
                        ml.quit();
                        return glib::ControlFlow::Break;
                    }
                    _ => say(&format!("unknown {line}")),
                }
            }
        });
    }
    say("ready");
    main_loop.run();
}
