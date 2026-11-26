# Plan: dictate unter niri (Wayland), Baustein 6 / SPEC-v2 Phase 6

> **Abgeschlossen am 23.11.2026:** W0–W7 samt W6-Nachzug gebaut, gemergt (`main` `6863c6a`), gepusht und installiert;
> Daniel hat W2–W6 am echten Desktop live abgenommen (Entscheidung 23, Abschnitt 7; Befund und „nicht belegt“ in 6h,
> „Live-Abnahme W2–W6“). Was unten folgt, ist die Geschichte der Blöcke.

Stand 20.11.2026. Bestandsaufnahme und Plan; am Produkt **noch nichts gebaut**. Daniel hat die Entscheidungen am 20.11.
getroffen (Abschnitt 7); die Abschnitte 3 und 6 sind danach ausgerichtet. Die Awesome-Teile bleiben lauffähig, bis die
Wayland-Variante live abgenommen ist, und werden dann entfernt (Block W7).

**W0 (Spikes) am 20.11. abgeschlossen:** S2, S3 und S4 im verschachtelten niri gemessen, S1, S3-Noctalia und S5 live
mit Daniel gemessen; Daniel hat die Ausgabe entschieden (Abschnitt 7, Punkt 8). Ergebnisse je Spike in `docs/spikes/wayland-*.md`, Spike-Code unter
`spikes/wayland-*/` (eigene Crates, nicht Teil von dictate). Die Befunde sind unten je Abschnitt als „Ergebnis W0“
eingetragen, die Folgen für die Blöcke in Abschnitt 6.

Kennzeichnung je Befund: **gemessen** (auf blub geprüft), **abgeleitet** (aus Code, Doku oder Binary geschlossen, nicht
ausprobiert), **offen** (nicht geprüft, Spike oder Live-Test nötig).

## 1. Bestandsaufnahme (20.11.2026)

### Umgebung

| Was | Befund | Art |
|---|---|---|
| Sitzung | niri 26.04 (`8ed0da4`), `XDG_SESSION_TYPE=wayland`, `WAYLAND_DISPLAY=wayland-1`, `NIRI_SOCKET` je Sitzung neu benannt | gemessen |
| Shell | Noctalia 5.5.0 (C++, Leiste, Benachrichtigungen, Sperre über ext-session-lock, Zwischenablage-Verlauf, Plugins in Luau mit eigenen Layer-Flächen ab API 36) | gemessen / Plugins abgeleitet aus niri-coach `docs/research.md` B1 |
| X11-Programme | über `xwayland-satellite :0` (z. B. Spotify); Firefox, Slack, Orca, Alacritty laufen nativ | gemessen |
| Werkzeuge | `wtype`, `ydotool`/`ydotoold` (Dienst aus), `wl-copy`/`wl-paste`, `grim`, `sway` (für kopflose Tests), `xdotool`/`xclip` (nur noch XWayland); kein `fcitx`/`ibus` | gemessen |
| Bibliotheken | `gtk4 4.24.1`, `gtk4-layer-shell 1.3.0` (System); im eigenen `Cargo.lock` schon `smithay-client-toolkit 0.20`, `wayland-client 0.31`, `wayland-protocols 0.32` (+ `-wlr`, `-misc`), `glutin 0.32`, `egui_glow 0.37` (über eframe/winit) | gemessen |
| Rechte | User in Gruppe `input` (liest `/dev/input/event*`, also jeden Tastendruck), ACL `blub:rw` auf `/dev/uinput` | gemessen |
| Portale | `xdg-desktop-portal` 1.22.1, `-gnome` 51.0 (Vorgabe in `niri-portals.conf`) und `-gtk` laufen; das Portal `org.freedesktop.portal.GlobalShortcuts` ist exportiert, aber **`BindShortcuts` scheitert** (das Backend ruft `org.gnome.Settings.GlobalShortcutsProvider`, den es auf niri 26.04 nicht gibt): **nicht nutzbar**; Messung vom 26.11.2026 in `PLAN-features.md` Abschnitt 1 (die frühere Fassung „kein GNOME-Portal-Backend“ war abgeleitet und ungenau) | gemessen |

**Wayland-Globals von niri** (Registry gelesen, nichts gebunden): `zwlr_layer_shell_v1 v5`, `zwp_virtual_keyboard_manager_v1`,
`zwp_input_method_manager_v2`, `zwp_text_input_manager_v3`, `ext_data_control_manager_v1`, `zwlr_data_control_manager_v1 v2`,
`zwp_primary_selection_device_manager_v1`, `wp_fractional_scale_manager_v1`, `wp_cursor_shape_manager_v1`,
`ext_session_lock_manager_v1`, `ext_idle_notifier_v1`, `ext_foreign_toplevel_list_v1`, `zwlr_foreign_toplevel_manager_v1`,
`zwp_keyboard_shortcuts_inhibit_manager_v1`, `xdg_activation_v1`, `ext_background_effect_manager_v1`. **gemessen**

**niri-Kürzel:** Kürzel lösen nur beim Drücken aus; `release=true` lehnt `niri validate` ab („unexpected property“)
**gemessen**; die Doku der Standard-Config nennt als Eigenschaften nur `repeat`, `cooldown-ms`, `allow-when-locked`,
`allow-inhibiting`, `hotkey-overlay-title` **abgeleitet**.
Ein zweiter `binds {}`-Block über `include "dictate-binds.kdl"` ist gültig (auch bei doppelter Taste) **gemessen**.
Die IPC sieht keine Tastendrücke (niri-coach `research.md` A4) **abgeleitet**.

**Kürzel heute belegt** (aus `coach-binds.kdl`, erzeugt von niri-coach aus dessen `catalog/actions.toml`, nicht von Hand
ändern): `Mod+Space` = Spalte als Tabs, `Mod+Shift+Space` = schwebend, `Mod+Ctrl+L` = Sperre (Noctalia),
`Mod+Shift+S` = Palette von niri-coach, `Mod+X` = Noctalia-Launcher. **Frei:** `Mod+E`, `Mod+Shift+E`, `Mod+Ctrl+E`,
`Mod+Ctrl+Shift+E`, `Mod+I`, `Mod+Shift+I`. **gemessen**

### dictate unter niri

| Teil | Befund | Art |
|---|---|---|
| `dictate.service` | läuft seit Boot (19.11. 16:00, vor niri), Umgebung ohne `DISPLAY`/`WAYLAND_DISPLAY`/`NIRI_SOCKET`; `dictate status` → `idle` | gemessen |
| Daemon-Kern | Whisper/CUDA, Socket-Protokoll, KI, Diagnose-Katalog brauchen kein Display | abgeleitet (Code) |
| Audio | Config-Quelle `alsa_input.usb-046d_Logitech_BRIO…` existiert unter PipeWire 1.6.11 (pulse), ist Standardquelle | gemessen; Aufnahme selbst offen (Live-Test) |
| CLI `start/stop/toggle/cancel/status/listen` | gehen an den Socket, displayunabhängig. **Getippt wird nicht vom Daemon**, sondern vom Zuhörer (`listen`, heute Awesome) | abgeleitet (Code) |
| `dictate act/palette/diag` (+ `dictate-palette`, `dictate-diag`) | Socket-Teil geht; Markierung lesen und Kopieren laufen über `xclip` → unter niri nur noch XWayland-Auswahl, nicht die der Wayland-Programme | abgeleitet |
| `dictate paste`, `synth`, `release-keys` | X11 (x11rb, XTest, XInput) → wirken nur auf XWayland-Programme; nicht ausgeführt | abgeleitet |
| Einstellungsfenster `dictate-settings` (egui, eframe mit Feature `wayland`) | läuft nativ als `xdg_shell`-Fenster (kopfloses sway, pixman, gegen den Mock). Text „Set in the Awesome module“ auf General passt nicht mehr | gemessen (nicht auf niri mit NVIDIA und Skalierung 1,625) |
| Awesome-Modul (Overlay, Kürzel, Panels Textaktionen/Palette/Diagnose, Tipp-Warteschlange, Sperre) | läuft unter niri nicht (kein Awesome) | gemessen |
| picom-Regeln | entfallen; Gegenstück sind niri `layer-rule`s | abgeleitet |
| Sperre | Noctalia ruft `SetLockedHint` auf (im Binary), die Wayland-Sitzung ist logind-Sitzung 4 | abgeleitet; ob `LockedHint` beim Sperren wirklich `yes` wird, offen |
| Terminal- und Profil-Erkennung | `actions.terminal_classes` und `profiles.*.match_class` sind X-Fensterklassen (`Alacritty`, `kitty`, `XTerm`, `URxvt`, `st-256color`, `org.wezfurlong.wezterm`, `foot`), verglichen ohne Groß/klein, aber exakt. Unter Wayland zählt die `app_id`; stimmt sie nicht, wird in ein Terminal **eingefügt** statt getippt (mehrzeiliger Text kann dort Befehle ausführen) | abgeleitet (Code); Alacritty: `app_id` = `Alacritty` gemessen |
| Fernzugriff | `wayvnc` läuft; Eingaben über VNC kommen nicht über `/dev/input` | gemessen (Prozess), Folge abgeleitet |

**Größe dessen, was neu gebaut werden muss:** das Awesome-Modul hat 10 764 Zeilen Lua (`actions.lua` 2 888,
`diagnose.lua` 2 265, `palette.lua` 1 912, `init.lua` 1 808, `palette_exec.lua` 768, Rest Hilfen). Ein Teil davon ist
Logik (Tipp-Warteschlange, Stückelung, Terminal-Entscheidung, Abbruch, Sitzungen der Panels), die in Rust testbarer
wird; der Rest ist Oberfläche, die neu gezeichnet wird. Eine Aufstellung je Modul (Logik nach Rust oder neu zeichnen)
ist Teil von Block W1.

## 2. Leitlinien

- **Rust-Kern bleibt, die Desktop-Schicht wird ein eigener Wayland-Prozess** `dictate-ui` (Arbeitsname) statt Awesome-Lua.
  Er hängt am Daemon wie heute Awesome (`listen`-Ereignisse, `act_*`, `pal_*`, `diag_*`), zeichnet Overlay und Panels
  und gibt Text aus. Der Daemon bekommt **keinen** Display-Bezug (seine Umgebung kennt die Wayland-Sitzung nicht, der
  niri-Socket heißt je Sitzung anders).
- `dictate-ui` startet als User-Dienst `dictate-ui.service` mit `PartOf=graphical-session.target` (niri-session
  importiert die Umgebung), damit er neu startet und ins Journal loggt.
- **X11/Awesome bleibt bis zur Live-Abnahme lauffähig** (danach W7: entfernen). Bis dahin kommt die Wayland-Ausgabe als
  eigenes Backend neben die X-Wege (`paste.rs`, `keys.rs`, `actions/selection.rs`), gewählt zur Laufzeit; die
  X-Schritte von `check.sh` bleiben grün.
- **Tests nur isoliert:** neuer Schritt `wayland` in `scripts/check.sh`: kopfloses sway (`WLR_BACKENDS=headless`), darin
  niri verschachtelt mit Test-Config (Muster aus niri-coach), eigener kurzer Laufzeitordner unter
  `/home/data/stt-test/wl-run` (Unix-Socket-Pfade über 108 Zeichen scheitern, gemessen), nie die echte Sitzung, nie
  `niri msg action` an das echte niri, keine uinput-Geräte in Tests (das echte niri würde sie als Tastatur übernehmen).

## 3. Teile mit Optionen und Empfehlung

### 3.1 Kürzel

| Option | Bewertung |
|---|---|
| **A. Eigene Datei `~/.config/niri/dictate-binds.kdl`, eingebunden mit `include`** | gemessen gültig; dictate pflegt sie selbst (aus dem Repo installiert); niri-coach kennt die Tasten dann nicht (keine Hilfe, kein Lernen) |
| B. Einträge im Katalog von niri-coach (`catalog/actions.toml`) | eine Quelle für alle Tasten, erscheinen in Hilfe und Palette von niri-coach; dictate hängt an einem fremden Repo |
| C. GlobalShortcuts-Portal | geht auf blub mit niri 26.04 nicht: das Backend läuft und exportiert die Schnittstelle, `BindShortcuts` scheitert (gemessen 26.11.2026, `PLAN-features.md` Abschnitt 1); niri hat die Gegenseite mit #4744 am 07.11.2026 gemergt, noch in keinem Release (Wiedervorlage K1b) |

Alle Einträge rufen nur `spawn` mit Argumentliste (nie `spawn-sh`), z. B. `Mod+E repeat=false { spawn "dictate" "start" "--hold"; }`
(`--hold` ist neu, gibt es noch nicht).
**Entschieden (2): A**, eigene `~/.config/niri/dictate-binds.kdl`, aus dem Repo installiert und per `include` in
`config.kdl` eingebunden (die Zeile in `config.kdl` setzt Daniel bzw. die Installation mit seiner Bestätigung).
Palette braucht eine neue Taste, weil `Mod+Space` belegt ist (offen, in W5).

### 3.2 Halten zum Sprechen (Mod+E halten)

niri meldet kein Loslassen (gemessen). Optionen:

| Option | Bewertung |
|---|---|
| **A. Drücken über niri-Kürzel, Loslassen über den Tastenzustand der Tastatur** (`EVIOCGKEY` auf `/dev/input/event*` alle ~20 ms, nur solange aufgenommen wird; nur der Zustand von `E` und Super, nie `read()` des Ereignisstroms) | gleiche Bedienung wie heute; braucht die Gruppe `input` (vorhanden, aber damit könnte der Prozess grundsätzlich jeden Tastendruck lesen: Code-Regel „nur `EVIOCGKEY`“ plus Test). Funktion nur live prüfbar |
| B. Umschalten statt Halten (Mod+E an, Mod+E aus) | keine neuen Rechte, sofort machbar; andere Gewohnheit. Freihändig (`Mod+Shift+E`) gibt es ohnehin |
| C. Overlay greift beim Drücken die Tastatur (Layer `exclusive`) und sieht das Loslassen | ohne Sonderrechte, aber das Zielfenster verliert kurz den Fokus (Textfelder, Terminals reagieren darauf) und niri unterdrückt das Loslassen der vom Kürzel verbrauchten Taste vermutlich; offen, Spike |

Wichtig: Die Frage ist nicht nur Halten oder Umschalten. Bei B drückt Daniel zum Beenden Mod+E, **Super ist also genau
dann gehalten, wenn die Ausgabe beginnt**; eine virtuelle Tastatur löste dann niri-Kürzel aus (`Mod+Shift+C` schließt
das Fenster, `Mod+Shift+Q` beendet niri). Jede Variante braucht deshalb das Warten auf losgelassene Modifikatoren
(Regel 2 in Abschnitt 4), und dafür gibt es nur zwei Quellen: den Tastenzustand über `input` (wie A) oder das, was niri
einem Client über den Tastaturfokus meldet (nur für ein fokussiertes Fenster, also nicht für die Ausgabe ins Zielfenster;
der Modifikator-Zustand, den das virtuelle Gerät sieht, ist offen, S2). Ohne `input` bleibt nur T3 (Eingabemethode,
keine Tastenereignisse) oder eine feste Wartezeit.

Über VNC (`wayvnc`) sieht A kein Loslassen: dort gilt eine Höchstdauer, danach stoppt die Aufnahme (Rückfall wie B).

**Entschieden (3): A.** dictate darf den Tastenzustand über die Gruppe `input` lesen, **nur mit `EVIOCGKEY`**, nie den
Ereignisstrom (Code-Regel plus Test). B bleibt Rückfall in der Config (kein Tastaturgerät lesbar, VNC nach Höchstdauer).

**Ergebnis W0 (`docs/spikes/wayland-release.md`):** Variante C **gemessen und verworfen**: ein Overlay, das beim Kürzel
die Tastatur greift, sieht das Loslassen von E nie (niri verschluckt die Kürzel-Taste), nur das von Super, und das
Zielfenster verliert den Fokus. A ist abgeleitet tragfähig (Kernel aktualisiert das Bitfeld unabhängig vom Lesen,
Latenz ≤ Abfrageabstand, mehrere Tastaturen je eigenes Bitfeld) und **live gemessen**: 22 Drücke von E mit
Loslassen erkannt (kürzester 81 ms), Abfrageabstand p99 24,6 ms, Maximum 35 ms, `strace` zeigt kein `read` auf ein
Eingabegerät. Geräte: Ducky (`event5`, `event9`), G903, Espanso (uinput). Der Bedarf „Warten auf Modifikatoren für die Ausgabe“ ist durch S2 kleiner geworden
(3.4).

### 3.3 Overlay und Panels (Oberfläche)

Anforderungen: Overlay nimmt nie den Fokus und lässt Klicks durch; Panels brauchen Texteingabe und gestreamtes Markdown;
Skalierung 1,625 auf NVIDIA; Kopplung an Noctalia gegen Portabilität (Phase 6); Design heute nur in egui umgesetzt.

| Option | Fokus/Klick-durch | Texteingabe, Markdown | Skalierung 1,625, NVIDIA | Kopplung | Design |
|---|---|---|---|---|---|
| **A. Rust + GTK 4 + gtk4-layer-shell** | Layer `overlay`, `keyboard none`, leere Eingaberegion; Panels `on_demand`/`exclusive` | reif (Eingabe, IME, Pango-Markup: `markdown.lua` lässt sich fast 1:1 übertragen) | GTK kann gebrochene Skalierung; niri-coach nutzt denselben Stapel auf blub (abgeleitet aus dessen Repo, hier nicht gemessen) | jeder Compositor mit Layer-Shell (niri, sway, Hyprland, KDE) | zweites Toolkit: Stil als CSS neu (seit Entscheidung 9: Noctalia-Palette statt Pushing Pixels) |
| B. Rust + smithay-client-toolkit + egui_glow (alle Crates schon im Lockfile) | Layer-Shell direkt | egui-Eingabe gut; Markdown selbst (oder Crate) | Glue selbst: EGL auf NVIDIA, `wp_fractional_scale`, xkbcommon, Cursor, kein IME | wie A | ein Toolkit, Stil aus `src/gui` wiederverwendbar |
| C. Noctalia-Plugin (Luau, `[[layer]]`/`[[panel]]`, `runStream` für `listen`) | Layer-Einträge mit `keyboard` und `click_through` | Texteingabe und Markdown in `ui.*` ungeklärt | macht Noctalia | **nur mit Noctalia**; `runStream` nimmt eine Shell-Zeile; Zeitbudgets je Aufruf | Noctalia-Theme statt Pushing Pixels |
| D. Overlay per Layer-Shell, Panels als normale eframe-Fenster mit niri-Fensterregel | Overlay wie B, Panels nehmen als Fenster den Fokus | egui | Panels: winit erledigt es; Overlay: Glue wie B | wie A | egui |

**Entschieden (4): A**, GTK 4 für `dictate-ui`; das Einstellungsfenster bleibt egui (läuft schon unter Wayland).
Gründe: Plattformnähe (gebrochene Skalierung, Fokus, Eingabe, Barrierefreiheit) wiegt bei einem dauernd vorhandenen
Overlay und Tastatur-Panels schwerer als ein einheitliches Toolkit; B müsste genau diese Teile selbst schreiben, C
bindet dictate an Noctalia. Preis: der Stil ein zweites Mal (CSS) und GTK als Systemabhängigkeit.

**GLSL-Effekte (Bedingung von Daniel):** niri wendet eigene Shader nur auf Fenster an (`window-open`, `window-close`,
`window-resize`; so auch in `~/.config/niri/animations.kdl`), nicht auf Layer-Flächen, unabhängig vom Toolkit
(abgeleitet aus Config-Kommentar und Binary). Daraus folgt die Aufteilung:

- **Overlay:** Layer-Fläche (Layer `overlay`, `keyboard none`, leere Eingaberegion, Namensraum `dictate-overlay` für
  `layer-rule` mit Blur und Schatten). Keine niri-Shader; Bewegung clientseitig in GTK (Animationen, bei Bedarf eigenes
  GLSL in `GtkGLArea`).
- **Panels (Textaktionen, Diagnose, Palette): schwebende GTK-Fenster** mit eigener `app_id` (z. B. `dictate-panel`) und
  niri-Fensterregel (`open-floating`, feste Größe, mittig): Daniels Shader beim Öffnen, Schließen und bei
  Größenänderung wirken, niri gibt den Fokus nach dem Schließen zurück. Nachteil: sie erscheinen in Übersicht und
  Fensterwechsel (S4 prüft, wie störend; Abhilfe über Fensterregeln, soweit niri sie bietet).

Spike S4 prüft beides in verschachteltem niri mit Skalierung 1,625, bevor W1 beginnt.

**Ergebnis W0 (`docs/spikes/wayland-gtk.md`, gemessen):** Overlay ohne Fokus, klick-durch mit leerer Eingaberegion,
über Vollbild sichtbar, keine niri-Shader; GTK nutzt `wp_fractional_scale` (Puffer 1040×195 für 640×120 logisch bei
1,625). Panel als schwebendes Fenster: Fensterregel greift, Fokus beim Erscheinen auch aus
einem langlebigen Prozess (GTK holt sich das Aktivierungs-Token selbst, nichts durchzureichen), Daniels `window-open`/`window-close`-Shader greifen, Fokus kehrt nach Schließen/Ausblenden
zum zuletzt fokussierten Fenster zurück. Neu: **`open-focused true` ist Pflicht**, sonst bleibt ein Panel über einem
Vollbildfenster unsichtbar und ohne Fokus. Der Resize-Shader greift nur, wenn niri die Größe setzt, nicht wenn GTK
wächst. Panels erscheinen in Übersicht und Alt+Tab; eine Fensterregel dagegen gibt es in niri 26.04 nicht (hinnehmen).

### 3.4 Text ausgeben (Tippen und Einfügen)

| Option | Bewertung |
|---|---|
| T1. `wtype` aufrufen | vorhanden; tauscht für Unicode die Tastaturbelegung des virtuellen Geräts; Abbruch mitten im Anschlag ohne Kontrolle, ob die Taste losgelassen wird |
| **T2. Eigene virtuelle Tastatur in Rust** (`zwp_virtual_keyboard_v1`, Crate schon im Lockfile) | wie wtype, aber im Prozess: Tempo, Abbruch mit garantiertem Loslassen (Ersatz für `release-keys`), Warten auf losgelassene Modifikatoren eingebaut |
| T3. Eingabemethode (`zwp_input_method_v2.commit_string`) | exakter Unicode-Text ohne Belegungstausch, **gehaltenes Super kann keine Kürzel auslösen** (kein Tastenereignis); nur in Programmen mit text-input-v3 (GTK, Qt, Firefox, Alacritty; Electron nur mit `--enable-wayland-ime`; XWayland nie); nur eine Eingabemethode je Sitz (heute keine aktiv) |
| T4. `ydotool` (uinput) | Keycodes abhängig von der Belegung, kein Unicode, Daemon nötig; nicht empfohlen |

Einfügen (außerhalb von Terminals, wie heute): `paste.rs` bekommt ein Wayland-Gegenstück über **Data-Control**
(Zwischenablage sichern, Text anbieten, `Ctrl+V` über T2, nach dem Abholen zurücklegen). Unterschied zu X: Data-Control
sagt nicht, **wer** den Text abholt; Noctalias Zwischenablage-Verlauf holt jeden neuen Inhalt sofort ab und würde
diktierten Text speichern. Gegenmittel (offen, S3): MIME-Typ `x-kde-passwordManagerHint: secret` mitgeben, damit Verläufe
ihn auslassen, und die Bestätigung anders bestimmen (z. B. zweite Abholung oder Frist). **Zurücklegen trägt alle
angebotenen Typen mit**, auch einen solchen Hinweis: legte die Wiederherstellung (wie `paste.rs` unter X) nur bekannte
Text-, Rich- und Bildtypen zurück, verlöre ein Passwort aus dem Passwortmanager seinen Hinweis und landete danach im
Verlauf.

Terminal-Entscheidung über `niri msg -j focused-window` (`app_id`, z. B. `Alacritty`; gemessen) statt der
X-Fensterklasse. Die Listen `actions.terminal_classes` und `profiles.*.match_class` bekommen die Wayland-`app_id`s
(z. B. `com.mitchellh.ghostty`, `org.gnome.Console`, `org.kde.konsole`), und **im Zweifel wird getippt, nicht eingefügt**
(Fokus oder `app_id` unbekannt).

**Entschieden (5):** T2 als Grundlage für Tippen und `Ctrl+V`, Einfügen über Data-Control; T3 im Spike S2 messen und,
falls in den Alltagsprogrammen zuverlässig, je Programm bevorzugen mit T2 als Rückfall (Ergebnis von S2 Daniel vorlegen).

**Ergebnis W0, T2/T3 (`docs/spikes/wayland-vkbd.md`, gemessen im verschachtelten niri, Kürzel mit Positivkontrolle):**

- T2 tippt Umlaute, ß, €, „“ exakt in GTK, Alacritty, xterm (XWayland), Firefox. Die virtuelle Tastatur **löst keine
  niri-Kürzel aus**, auch nicht als Mod-Akkord, und **erbt keine physisch gehaltenen Modifikatoren** (Programm sieht
  `modifiers(0)`, Terminals bekommen ein schlichtes Zeichen statt `^C`/`ESC c`).
- **niri lässt beim Zerstören der virtuellen Tastatur oder beim Prozessende nicht los**: das Zielprogramm wiederholt
  die Taste bis zum nächsten Tastenereignis. Loslassen ist Pflicht (Regel 3).
- **Chromium deutet evdev-Code 14 (Backspace) als Rücktaste**, auch wenn die erzeugte Belegung dort `:`/`,`/`-` trägt:
  die Belegung von T2 darf Zeichen nur auf Codes ohne Sonderbedeutung legen.
- T3 (Eingabemethode) exakt in GTK, Alacritty, Firefox, Chromium 157 (auch **ohne** IME-Flags); xterm (XWayland)
  aktiviert sie nicht. Slack (Electron 45/Chrome 156) abgeleitet wie Chromium, Orca (Electron 43/Chrome 150) offen,
  beide live in W2. Eine **zweite Eingabemethode am Sitz bleibt stumm**, auch nach dem Ende der ersten: dictate hält
  seine nur für die Dauer einer Ausgabe.
- **Entschieden (Daniel, 20.11., Punkt 8 in Abschnitt 7): T3 automatisch**, wenn das Zielfeld sie aktiviert, sonst T2
  (Tabelle je Programm in `docs/spikes/wayland-vkbd.md`).

**Ergebnis W0, Einfügen (`docs/spikes/wayland-paste.md`, gemessen):** Sichern und Zurücklegen aller Typen samt
`x-kde-passwordManagerHint` byte-genau (7 Typen, 5 MB in 0,16 s), Ctrl+V über T2 kommt an. Ein gieriger Verlauf holt
den Text **vor** dem Ctrl+V; als Bestätigung zählt nur eine Abholung **nach** dem Ctrl+V. Mit dem Hinweis lässt ein
hinweisbeachtender Verlauf den Text aus; **Noctalias Verlauf beachtet den Hinweis** (live gemessen). **Die
Auswahl ist leer, sobald der zurücklegende Prozess endet**: `dictate-ui` bedient zurückgelegte Inhalte selbst weiter
(auch gesicherte Passwörter; Begrenzung für Inhalte mit Hinweis in W2 entscheiden).

### 3.5 Markierung lesen (Textaktionen) und Kopieren

PRIMARY über Data-Control im Prozess (kein Fokus nötig; `wl-paste -p` zeigt, dass es geht), Kopieren über Data-Control
in CLIPBOARD. Data-Control kann CLIPBOARD jederzeit lesen: die Regel „nur PRIMARY“ steht deshalb im Code (eigene
Funktion je Quelle, CLIPBOARD-Lesen nur für das Sichern beim Einfügen und die Palette-Absicht `explain_clipboard`), mit
Test. Kein Wechsel nötig, nur Backend neben `xclip`.

### 3.6 Sperre

`lock.lua` löscht heute Sitzungen beim Sperren über xss-lock. Unter niri sperrt Noctalia (ext-session-lock) und setzt
nach Binary-Befund `LockedHint` in logind. Plan: `dictate-ui` (oder der Daemon über den System-Bus) beobachtet
`LockedHint` der eigenen logind-Sitzung und schickt dann `act_clear`/`diag_clear`. Offen (S5): ob `LockedHint` beim
Sperren wirklich umspringt.

**Ergebnis W0 (`docs/spikes/wayland-lock.md`):** Wayland-Sitzung ist heute logind-Sitzung 4 (`Type=wayland`,
`LockedHint=no`, gemessen); Noctalia ruft `SetLockedHint` auf `/org/freedesktop/login1/session/auto` (abgeleitet).
**Live gemessen:** `LockedHint` springt beim Sperren über Noctalia auf `yes` (mit `PropertiesChanged`-Signal) und
beim Entsperren zurück. Nebenbefund: Entsperren mit Passwort scheiterte nach einem Noctalia-Update im laufenden
Betrieb („PAM start failed“), `loginctl unlock-session` half. Die Sitzung findet
`dictate-ui` über `XDG_SESSION_ID` bzw. `Display` des Users, nie über eine feste Nummer.

### 3.7 Befehlspalette

Viele Absichten sind Awesome-spezifisch (Tags, `sticky`, `ontop`, Fenster per Awesome-Client, Widgets `doorcam`,
`claude_usage`). Unter niri überschneidet sie sich mit der Palette von niri-coach (`Mod+Shift+S`) und dem
Noctalia-Launcher (`Mod+X`). Optionen: (a) portieren mit niri-Absichten (`niri msg action`, Fenster aus
`niri msg -j windows`, Programme aus Desktop-Einträgen, System über `noctalia msg`), (b) auf KI-/Text-Absichten
reduzieren (`ask`, Zwischenablage erklären, Diagnose, Einstellungen) und den Rest den vorhandenen Werkzeugen überlassen,
(c) zurückstellen.

**Entschieden (6): (a), voll auf niri portieren.** Jede Absicht bekommt eine niri-Umsetzung: Fenster und Arbeitsflächen
über niri-IPC (`niri-ipc` exakt auf die niri-Version gepinnt, wie in niri-coach), Programme aus Desktop-Einträgen,
System und Widgets über `noctalia msg` oder vorhandene Programme. Absichten ohne Gegenstück unter niri (z. B. `sticky`,
`ontop`, Awesome-Widgets) werden in W5 einzeln Daniel vorgelegt (ersetzen oder streichen), nicht still entfernt. Die
Sicherheitsregeln der Palette bleiben: fester Katalog, lokale Auflösung, nur argv, Modelltext nie als Befehl. Neue
Taste statt `Mod+Space` wählt Daniel in W5.

## 4. Sicherheitsregeln in Wayland-Begriffen

Alle Regeln aus `ROADMAP.md` und `CLAUDE.md` gelten weiter. Was sich durch Wayland ändert:

1. **Nur PRIMARY**, CLIPBOARD nur für die zwei bekannten Ausnahmen: Data-Control macht beides ohne Fokus lesbar, die
   Grenze liegt allein im Code (3.5).
2. **Kein synthetischer Anschlag bei gehaltenem Modifikator.** Befürchtet war, dass eine virtuelle Tastatur bei
   gehaltenem Super niri-Kürzel auslöst (z. B. `Mod+Shift+C`, `Mod+Shift+Q`). **S2 (gemessen, verschachtelt):** die
   virtuelle Tastatur läuft an niris Kürzeln vorbei und hat einen eigenen Modifikator-Zustand; das virtuelle Gerät
   selbst sieht keinen. Die Regel bleibt trotzdem als Vorsicht, bis der DRM-Live-Test in W2 das bestätigt: vor jedem
   Auftrag warten, bis Super/Ctrl/Alt/Shift losgelassen sind (Quelle: Tastenzustand wie 3.2 A), Pflicht für `Ctrl+V`.
3. **Abbruch lässt alles los.** Ersatz für `release-keys`: T2 drückt und lässt jede Taste in einem Zug los und sendet
   vor dem Beenden für jede gedrückte Taste das Loslassen. **S2 (gemessen):** niri lässt beim Zerstören oder beim
   Prozessende **nicht** los; das Zielprogramm wiederholt die Taste dann bis zum nächsten Tastenereignis.
4. **Kürzel nur mit Argumentlisten** (`spawn`, nie `spawn-sh`), Panels starten nichts über eine Shell.
5. **Gruppe `input`** wird nur genutzt, wenn Daniel 3.2 A wählt; dann nur `EVIOCGKEY`, nie den Ereignisstrom lesen, nie
   uinput für Tests.
6. **Diktierter und eingefügter Text nicht in den Verlauf der Zwischenablage** (3.4; neu: unter X durfte clipmenud den
   Text abholen), beim Zurücklegen alle Typen samt Geheimnis-Hinweis mitnehmen. Markierung, Antworten und
   Diagnose-Berichte weiter nie loggen. Zurückgelegte Inhalte (auch Passwörter) leben danach im Speicher von
   `dictate-ui`, weil die Auswahl sonst leer wäre (S3); nie loggen, nie an den Daemon.
7. **Sperre löscht Sitzungen** wie heute, neues Signal (3.6).
8. **Im Zweifel tippen statt einfügen** (unbekannte `app_id`), damit kein mehrzeiliger Text in einem Terminal landet.
9. **Eingabemethode nur auf Zeit** (S2): eine dauerhaft gehaltene Eingabemethode legte jede spätere (fcitx5, ibus)
   still lahm.

## 5. Spikes vor dem Bau

Je Spike höchstens zwei Ansätze, dann anhalten und melden; Ergebnis als Datei `docs/spikes/wayland-<name>.md`
(Ordner neu), Tests nur in verschachteltem niri, außer wo „live“ steht.

| Spike | Frage | Budget | Wo |
|---|---|---|---|
| S1 Loslassen | Erkennt `EVIOCGKEY` das Loslassen von Mod+E zuverlässig (Latenz, mehrere Tastaturen)? Höchstdauer für VNC; Variante C ansehen | 1 h | live mit Daniel (nur lesen) |
| S2 Virtuelle Tastatur | Umlaute und `ß` in Alacritty, Firefox, Slack, Orca, einem XWayland-Programm; Verhalten bei gehaltenem Super; Abbruch mitten im Anschlag; T3 (Eingabemethode) in denselben Programmen | 2 h | verschachteltes niri, Programme dort live mit Daniel |
| S3 Einfügen | Data-Control sichern/anbieten/zurücklegen (alle Typen samt Hinweis); Noctalia-Verlauf und `x-kde-passwordManagerHint` | 1,5 h | verschachteltes niri; Verlauf live mit Daniel (nur Typen ansehen, nie Inhalte) |
| S4 GTK-Oberfläche | Overlay als Layer-Fläche ohne Fokus und klick-durch; Panel als schwebendes Fenster mit Eingabefeld (niri-Shader greifen, Fensterregel, Auftauchen in Übersicht/Fensterwechsel); Skalierung 1,625; Fokus kehrt nach dem Schließen zum Zielfenster zurück | 2 h | verschachteltes niri |
| S5 Sperre | Springt `LockedHint` beim Sperren über Noctalia? | 15 min | live mit Daniel |

Stand W0 (20.11.):

| Spike | Ergebnis | Datei | Offen |
|---|---|---|---|
| S1 | Variante C gemessen und verworfen; A abgeleitet tragfähig; Werkzeug `keystate` bereit | `docs/spikes/wayland-release.md` | live gemessen; offen nur zweite Tastatur und `ioctl`-Kosten ohne `strace` |
| S2 | T2 und T3 gemessen in GTK, Alacritty, xterm, Firefox, Chromium; Kürzel und Modifikatoren unkritisch, kein Loslassen beim Zerstören, Chromium-Code-14-Falle, zweite Eingabemethode stumm | `docs/spikes/wayland-vkbd.md` | entschieden: T3 automatisch; Slack/Orca und DRM live in W2 |
| S3 | Sichern/Anbieten/Ctrl+V/Zurücklegen mit allen Typen samt Hinweis gemessen; Auswahl leer nach Ende des Anbieters | `docs/spikes/wayland-paste.md` | Noctalia beachtet den Hinweis (live); offen: stellt Noctalia eine Auswahl nach Ende des Besitzers wieder her |
| S4 | Overlay und Panel wie geplant gemessen; `open-focused true` nötig; Resize-Shader nur bei Größe durch niri | `docs/spikes/wayland-gtk.md` | Leistung auf NVIDIA (W1), Panels in Übersicht/Alt+Tab (Daniel) |
| S5 | `LockedHint` springt beim Sperren und Entsperren (live, mit Signal) | `docs/spikes/wayland-lock.md` | – |

Isolation aller Messungen: Harness `spikes/wayland-nested/harness.sh` (seit W1 `tests/wayland/nested/harness.sh`) (kopfloses sway, pixman, verschachteltes niri,
Laufzeitordner unter `/home/data/stt-test/wl-run`, jeder Abbau ohne Restprozess). Grundlage für den `check.sh`-Schritt
`wayland` in W1.

## 6. Blöcke (je Block eine frische Sitzung, eigener Branch)

| Block | Inhalt | Prüfung |
|---|---|---|
| W0 | Spikes S1–S5, Entscheidungen festhalten | Spike-Dateien |
| W1 | `dictate-ui` Gerüst: Dienst, Daemon-Anbindung (`listen`), Overlay (Layer `overlay`, ohne Fokus), niri-Kürzel, Halten/Umschalten, Abbruch; Aufstellung der Lua-Module; `check.sh`-Schritt `wayland` | `fmt,clippy,unit,wayland` |
| W2 | Ausgabe: Tipp-Warteschlange (aus `init.lua`), virtuelle Tastatur, Warten auf Modifikatoren, Abbruch mit Loslassen, Einfügen über Data-Control, Terminal-Erkennung über niri-IPC | `fmt,clippy,unit,wayland`, live |
| W3 | Textaktionen: PRIMARY, Panel mit gestreamtem Markdown, Copy/Replace, Sperre | dito |
| W4 | Diagnose-Panel (gleiche Panel-Bausteine), Hinweis nach unsauberem Boot | dito |
| W5 | Palette voll auf niri portiert (niri-IPC, Desktop-Einträge, `noctalia msg`), neue Taste; Absichten ohne Gegenstück einzeln vorlegen. **Auftrag in 6g** (Blöcke A–E, Vorlagen E1–E12 offen) | dito |
| W6 | Einstellungsfenster in Noctalia-Farben (Entscheidung 9), Kürzel-Text für niri, Dateikollision `palette.json`, `DESIGN.md`, README, CLAUDE.md, ROADMAP, Installation, Live-Abnahme. **Auftrag in 6h** (Blöcke A–E, Vorlagen V1–V5). **Fertig, installiert, live abgenommen am 23.11.** (Entscheidung 23) | volle Suite |
| W7 | **W7a und W7b fertig, gemergt (6f).** Vorgezogen (Entscheidung 15, 6f), sonst nach der Live-Abnahme: Awesome-Modul, X11-Wege (`paste.rs`-X-Teil, `keys.rs`, `synth`, `release-keys`, `xclip`), Xephyr-Schritte von `check.sh`, Awesome-Doku und `x11rb` entfernen | volle Suite |

**Folgen aus W0 für die Blöcke:**

- **W1:** Harness aus `spikes/wayland-nested/` als `check.sh`-Schritt `wayland` übernehmen (F24 zum Nachschieben
  statt Shift, Laufzeitordner unter `wl-run`, Abbau per PID/Prozessgruppe). Overlay: leere Eingaberegion nach jedem
  Map setzen. Installierte KDL bekommt neben den Kürzeln die Regeln `window-rule` für `dictate-panel`
  (`open-floating true`, `open-focused true`) und `layer-rule` für `dictate-overlay`. Halten: EVIOCGKEY nur während
  einer Aufnahme, Ende bei losgelassenem E (Entprellung), Höchstdauer; Live-Zahlen aus S1 vor W1 eintragen. Zeit bis
  zum ersten Bild von GTK auf NVIDIA messen.
- **W2:** T2-Belegung nur auf evdev-Codes ohne Sonderbedeutung, einmal je Auftrag hochladen, jede Taste in einem Zug
  drücken und loslassen, Loslassen vor `destroy` und bei `SIGTERM`. T3 je nach Entscheidung (Empfehlung: automatisch
  bei `activate`, Eingabemethode nur auf Zeit). Einfügen: Hinweis `secret` immer, Bestätigung nur durch Abholung nach
  dem Ctrl+V, zurücklegen mit allen Typen, `dictate-ui` bedient weiter; Grenze für Inhalte mit Hinweis entscheiden.
  Live auf DRM: Super gehalten + T2, Slack und Orca mit T3.
- **W3:** `Replace` prüft vor dem Schreiben, ob noch das beim Öffnen gemerkte Fenster fokussiert ist (niri gibt den
  Fokus an das zuletzt fokussierte Fenster zurück, das kann ein anderes sein). Panelgröße fest mit Scrollbereich oder
  über niri-IPC (nur dann greift der Resize-Shader). Sperre nach S5.

Bis W7 bleiben die X-Schritte von `check.sh` grün. Schätzung grob, nach W1 gemessen nachzuschärfen: W0 eine Sitzung,
W1 und W2 je ein bis zwei, W3 und W4 je eine bis zwei, W5 zwei bis drei, W7 eine.

## 6a. Auftrag W1 (nächste Sitzung, frisch)

Daniel hat am 20.11. entschieden: Reihenfolge wie geplant (W1, dann W2), W1 in einer frischen Sitzung. Diktieren unter
niri geht erst nach W2 wieder; bis dahin läuft nur der Daemon (`dictate start/stop/toggle` über den Socket).

- **Branch:** `wayland-w1` von `wayland-w0-spikes` (enthält Harness und Spike-Ergebnisse), eigener Orca-Worktree.
- **Ziel:** neues Binary `dictate-ui` (GTK 4 + gtk4-layer-shell, Toolchain 1.95.0, Crates wie in
  `spikes/wayland-gtk/Cargo.toml`), User-Dienst `dictate-ui.service` (`PartOf=graphical-session.target`), Anbindung an
  den Daemon über `listen` (Zustand, Pegel, Teilergebnisse, Fehler), Overlay als Layer-Fläche (`dictate-overlay`,
  `KeyboardMode::None`, leere Eingaberegion nach jedem Map), `dictate-binds.kdl` im Repo (Mod+E halten, Mod+Shift+E
  freihändig, Mod+Ctrl+E abbrechen, Mod+Ctrl+Shift+E Einstellungen; nur `spawn` mit argv) samt `window-rule` für
  `dictate-panel` (`open-floating`, `open-focused`) und `layer-rule` für das Overlay; Halten über EVIOCGKEY (nur
  `ioctl`, nur während einer Aufnahme, Ende bei losgelassenem E mit 80 ms Entprellung, Höchstdauer als Rückfall,
  Umschalten als Config-Rückfall); Abbruch. Aufstellung der Lua-Module (Logik nach Rust oder neu zeichnen) als
  Abschnitt in diesem Plan. Textausgabe ist **nicht** Teil von W1 (W2).
- **Abnahme:** `scripts/check.sh` hat den Schritt `wayland` (Harness aus `spikes/wayland-nested/`, F24 zum
  Nachschieben, Laufzeitordner unter `/home/data/stt-test/wl-run`, Abbau ohne Restprozess) und er ist grün mit einem
  Test für Overlay ohne Fokus/klick-durch und Halten/Loslassen gegen einen Mock-Daemon (eigener Socket, eigene
  Config); Test „nur EVIOCGKEY, nie read“ für das Tastenmodul; X-Schritte von `check.sh` bleiben grün. Live-Teil mit
  Daniel: Overlay unter niri auf NVIDIA (Zeit bis zum ersten Bild, Skalierung 1,625), Halten und Loslassen.
- **Prüfung:** `scripts/check.sh --only fmt,clippy,unit,wayland` während der Arbeit, volle Suite einmal am Ende.
- **Isolation:** wie W0 (nie `niri msg action` an das echte niri, nie `~/.config/niri` oder `~/.config/dictate`
  schreiben; die Installation von `dictate-binds.kdl` und die `include`-Zeile setzt Daniel bzw. nur mit seiner
  Bestätigung; `dictate.service` nicht anfassen).
- **Budget:** eine bis zwei Sitzungen; Halt nach zwei erfolglosen Ansätzen je Teilproblem, dann melden.
- **Bericht:** 5–12 Zeilen (Ergebnis, Commits, Prüfergebnis, Abweichungen, Offenes).

**Ergebnis W1 (20.11.2026, Branch `wayland-w1`):**

- Gebaut: `dictate-ui` (`src/bin/dictate-ui/`: `app.rs` Hauptthread, `link.rs` Daemon und Steuer-Socket,
  `overlay.rs` GTK), GTK-freie Teile mit Unit-Tests in `src/ui/` (`hold.rs` Haltelogik, `keys.rs` nur `EVIOCGKEY`,
  `fake_keys.rs` nur für Tests, `ctl.rs` Steuer-Socket, `view.rs` Overlay-Modell, `palette.rs` Noctalia-Palette),
  Feature `ui` (Vorgabe), `systemd/dictate-ui.service`, `niri/dictate-binds.kdl`, `noctalia/dictate.json.tmpl`,
  Schritt `wayland` in `scripts/check.sh` (`tests/wayland/run.sh`, Harness und Injektor nach `tests/wayland/nested/`
  verschoben, Mock-Daemon, GTK-Testfenster), Aufstellung der Lua-Module (6b).
- Messung: `dictate-ui hold` (kurzer Aufruf aus dem Kürzel) startet in 24 ms (Median von 30, Maximum 30 ms; 114
  Bibliotheken über GTK, kein CUDA/Whisper/Pulse). Dauer von W1: eine Sitzung, etwa 3 h einschließlich zweier voller
  Suiten (Schätzung: ein bis zwei Sitzungen). Unit gegen systemd geprüft (`systemd-analyze --user verify`),
  `graphical-session.target` ist in Daniels niri-Sitzung aktiv, `WAYLAND_DISPLAY` steht in der Umgebung des
  User-Managers.
- Prüfung: 27 Unit-Tests in `src/ui/`, Schritt `wayland` 42 Prüfungen grün (Gegenprobe: ohne leere Eingaberegion
  scheitern alle drei Klick-Prüfungen); Abbruch von `check.sh` mitten im Schritt lässt keinen Prozess und keinen
  Laufzeitordner zurück. Verschachtelt (llvmpipe) erstes Bild 1 s nach Prozessstart, Einblenden 160–480 ms: nicht
  aussagekräftig, live messen.
- Unabhängige Durchsicht (Sicherheit, Richtigkeit): nichts Gravierendes; umgesetzt: kleine Dateien (Palette,
  `--fake-keys`) nur als gewöhnliche Datei über `fstat` am geöffneten Deskriptor (`ui::fsutil`, nie Gerät oder FIFO),
  Steuer-Socket mit Sperrdatei (`ui.sock.lock`, `flock`) und Prüfung des Ordners (gehört dem User, 0700), `Hold::reset`
  behält die Höchstdauer, ein spät gescheitertes `start` setzt nur sein eigenes Halten zurück. Offen (nicht geprüft):
  schließt niri die Layer-Fläche (Ausgang aus), ob das nächste Einblenden sie wieder anlegt.
- Nebenbefund `check.sh`: unter niri öffnete der Schritt `xephyr` `dictate-settings` auf dem **echten Desktop**
  (eframe nimmt Wayland vor X11, nur `DISPLAY` war entfernt; Prüfung „Einstellungsfenster schwebend“ scheiterte
  darum). `check.sh` entfernt jetzt auch `WAYLAND_DISPLAY`, `WAYLAND_SOCKET`, `NIRI_SOCKET`, `SWAYSOCK`.
- Volle Suite am Ende: fmt, clippy, unit, xclip, wayland, xephyr, gui, speech grün (gui nach einem Kaltstart-Zeitlimit
  der ersten Seite im zweiten Lauf). **gpu rot, Ursache Umgebung, nicht W1:** `quiet_brio_over_pulse` (Brio −10 dB)
  liefert leeren Text, weil WirePlumber für `application.name:paplay` eine Kanallautstärke von 0,038 wiederherstellt
  (`~/.local/state/wireplumber/stream-properties`; gemessen: ein `paplay` in eine eigene Nullsenke läuft mit
  −28,4 dB); Code und Abhängigkeiten von Audio/Daemon sind gleich wie auf `main`. Vorschlag (eigener kleiner Branch): Test-Wiedergabe mit fester Lautstärke und ohne Wiederherstellung
  (`paplay --volume=65536` plus Stream-Eigenschaft gegen Restore) oder den Eintrag mit Daniels Zustimmung zurücksetzen.
- **Abweichungen vom Auftrag:**
  1. Die Kürzel rufen `dictate-ui hold|toggle|cancel` auf (kurzer Aufruf ohne GTK, eine Zeile an
     `$XDG_RUNTIME_DIR/dictate/ui.sock`), nicht `dictate start --hold`: das Halten beobachtet der langlebige Prozess
     (Tastenzustand, Overlay, in W5 die Stimme der Palette), der Daemon bleibt ohne Display-Bezug. `cancel` geht ohne
     laufende Oberfläche direkt an den Daemon. `Mod+Ctrl+Shift+E` startet `dictate-settings` direkt.
  2. Rückfall Umschalten **automatisch**: Sieht die erste Abfrage nach dem Kürzel E nicht gedrückt (über VNC, oder ein
     Druck unter etwa 40 ms) oder ist kein Gerät lesbar, läuft die Aufnahme als Schalter bis zum nächsten Mod+E,
     mit Hinweis im Overlay (`hold::GRACE` 150 ms). Als Config-Rückfall reicht es, in `dictate-binds.kdl` `hold` durch
     `toggle` zu ersetzen; ein eigenes Config-Feld gibt es nicht.
  3. Höchstdauer: die des Daemons (`max_listen_s`, Vorgabe 120 s) plus 5 s; bei `max_listen_s = 0` 10 min. Danach
     `stop` und die Geräte werden geschlossen.
  4. Ende nur am Loslassen von E (wie 6a und S1); Awesome beendete auch beim Loslassen von Super (6b, Befund 4).
  5. Design nach Entscheidung 9 (Noctalia-Palette) statt Pushing Pixels.
- Nicht in W1 (wie geplant): Textausgabe (ein fertiger Text wird nur gezählt, Länge im Log), Profil nach `app_id`
  (`start` ohne `app`; W2 mit niri-IPC), Stimme der Palette (W5).
- **Installiert am 20.11. (Daniel: „Installier es, ich teste live“):** `~/.cargo/bin/dictate-ui`,
  `dictate-ui.service` (aktiviert, `graphical-session.target`), `~/.config/niri/dictate-binds.kdl` plus `include` in
  `config.kdl` (chezmoi-Quelle geändert und angewendet, Sicherung `config.kdl.pre-dictate-2026-11-20`), Vorlage
  `~/.config/noctalia/templates/dictate.json.tmpl` plus Block in `config.toml` (chezmoi, Sicherung
  `config.toml.pre-dictate-2026-11-20`), Palette geschrieben. `dictate.service` unverändert.
- **Live abgenommen (Daniel, 20.11., echtes niri auf NVIDIA, Skalierung 1,625):** Mod+E halten/loslassen, kurz
  loslassen und wieder drücken (bleibt gehalten), kurzer Tipper von 80 ms (als Halten erkannt, nicht als Schalter),
  Mod+Shift+E an/aus, Mod+Ctrl+E bricht ab (`final: cancelled` 11–29 ms danach), Mod+Ctrl+Shift+E öffnet
  `dictate-settings`; Fokus bleibt im Fenster, Klick durch das Overlay. Zahlen aus dem Journal:

  | Was | erster Live-Test | nach Korrektur (`e0e9196`) |
  |---|---|---|
  | Tastaturen öffnen | 145 ms (alle 26 Geräte, je 5–12 ms) | 0,5–0,6 ms (nur 4 mit E laut sysfs, Hilfsthread) |
  | erstes Einblenden | 286 ms (Vulkan-Swapchain beim ersten Map) | 11 ms (Vorwärmen beim Start: 228 ms nach Prozessstart) |
  | weitere Einblendungen | 1–7 ms | 2–5 ms |
  | Kürzel → Overlay sichtbar | ≈ 485 ms | 73 ms (inkl. Zustand vom Daemon) |
  | E losgelassen → `stop` | – | 100 ms (80 ms Entprellung plus Abfragetakt) |

  Einmal je Sitzung `VK_ERROR_OUT_OF_DATE_KHR` von GTK, wenn das Overlay mit der Mitschrift wächst (harmlos, neue
  Swapchain; beobachten, ob es flackert).
- **Offen, braucht Daniel:**
  1. ~~Installation~~ (erledigt, siehe oben; die Änderungen der chezmoi-Quelle sind dort noch nicht committet): `cargo install --path . --locked --bin dictate-ui` (installiert nur
     `dictate-ui`, der laufende Daemon bleibt), `systemd/dictate-ui.service` nach `~/.config/systemd/user/`, dann
     `systemctl --user daemon-reload` und `systemctl --user start dictate-ui`; `niri/dictate-binds.kdl` nach
     `~/.config/niri/` und in `config.kdl` die Zeile `include "dictate-binds.kdl"`; in `~/.config/noctalia/config.toml`:

     ```toml
     [theme.templates.user.dictate]
     input_path  = "/home/git/blub/dictate/noctalia/dictate.json.tmpl"   # nach dem Merge; vorher der Worktree
     output_path = "/home/blub/.local/state/dictate/palette.json"
     ```

     danach `noctalia msg templates-apply`. `config.kdl` und `config.toml` sind per chezmoi verwaltet.
  2. ~~Live-Test~~ (erledigt, siehe oben): Mod+E halten, sprechen, loslassen (Overlay unten, Fokus bleibt im Fenster, Klick durch das Overlay);
     erst E oder erst Super loslassen; Mod+Ctrl+E; Mod+Shift+E. Zahlen aus dem Journal:
     `journalctl --user -u dictate-ui | grep -E 'GTK ready|first frame|shown in'` (Zeit bis zum ersten Bild auf
     NVIDIA, Skalierung 1,625). Optional `strace -f -e trace=read,ioctl -p <pid>` während eines Haltens: keine `read`
     auf `/dev/input`. Getippt wird erst nach W2.
  3. ~~Befund 4 aus 6b~~: entschieden (Daniel, 20.11.): Ende nur am Loslassen von E, Super egal (so lassen).
  4. Erledigt am 20.11.: chezmoi-Quelle committet (Dotfiles-Branch `dictate-wayland`, nach `master` gemergt, nicht
     gepusht); Design nach Entscheidung 9 bestätigt; Tests spielen über Pulse mit fester Lautstärke ab
     (`tests/pulse_common`: `paplay --volume=65536 --property=state.restore-props=false`). Nebenwirkung des Versuchs
     dazu: WirePlumber speichert für `application.name:paplay` jetzt 1,0 statt 0,038, dazu ein Eintrag `dictate-test`.

## 6c. Auftrag W2 (nächste Sitzung, frisch)

Daniel hat am 20.11. W1 live abgenommen; `wayland-w1` wird erst nach W2 nach `main` gemergt. Nach W2 diktiert dictate
unter niri wieder.

- **Branch:** `wayland-w2` von `wayland-w1`, eigener Orca-Worktree (neuer Worktree: CUDA-Erstbau ca. 10 min, allein).
- **Ziel:** Textausgabe in `dictate-ui` (Abschnitt 3.4, Regeln 2, 3, 6, 8, 9 in Abschnitt 4, Befunde 1–3 und 12 in 6b):
  1. Tipp-Warteschlange aus `init.lua` nach Rust ohne GTK (`src/ui/`): Stückelung, ein Auftrag zur Zeit, Abbruch mit
     Mod+Ctrl+E leert sie, spätes `final` nach Abbruch wird verworfen, Tasten aus `final.keys` (nur `BackSpace`).
  2. T3 Eingabemethode (`zwp_input_method_v2`, nur für die Dauer einer Ausgabe), Commit, wenn das Zielfeld sie
     aktiviert; sonst T2 eigene virtuelle Tastatur (`zwp_virtual_keyboard_v1`: Belegung nur auf evdev-Codes ohne
     Sonderbedeutung, einmal je Auftrag hochladen, jede Taste in einem Zug drücken und loslassen, Loslassen vor
     `destroy` und bei `SIGTERM`). Config-Liste für Ausnahmen (Programme, die immer T2 bekommen).
  3. Vor jedem Auftrag warten, bis Super/Ctrl/Alt/Shift losgelassen sind (Tastenzustand über `ui::keys`, Geräte nur
     dafür kurz offen; Pflicht vor `Ctrl+V`).
  4. Einfügen außerhalb von Terminals über Data-Control: Zwischenablage mit allen Typen sichern, Text mit Hinweis
     `x-kde-passwordManagerHint: secret` anbieten, `Ctrl+V` über T2, Bestätigung nur durch eine Abholung nach dem
     `Ctrl+V`, alle Typen zurücklegen, `dictate-ui` bedient Zurückgelegtes weiter. **Entscheidung Daniel vorher:**
     Inhalte mit Hinweis `secret` höchstens 30 s weiter bedienen (Vorschlag aus S3), dann freigeben?
  5. Terminal-Erkennung über niri-IPC (`niri-ipc` exakt auf die niri-Version gepinnt, wie niri-coach; `app_id` des
     fokussierten Fensters), **im Zweifel tippen** (Befund 1: Lua fügte bei unbekanntem Fokus ein). Listen
     `actions.terminal_classes` und `profiles.*.match_class` um Wayland-`app_id`s ergänzen; `start` mit `app` für
     das Profil.
  6. Inhalte nie loggen (nur Längen), Zurückgelegtes nie an den Daemon.
- **Abnahme:** Schritt `wayland` erweitert: GTK-Zielfeld mit text-input-v3 (T3), Alacritty oder XWayland-`xterm`
  (T2), Umlaute/ß/€/„“ exakt, Einfügen samt Zurücklegen aller Typen (ohne Hinweis `secret`; mit Hinweis wird nach
  Entscheidung 10 getippt und die Zwischenablage nicht angefasst), Abbruch mitten im Tippen lässt alle
  Tasten los, kein Anschlag bei gehaltenem Modifikator (Positivkontrolle wie in S2). Unit-Tests für Warteschlange und
  Terminal-Entscheidung. Live mit Daniel: Diktat in Firefox, Alacritty, Slack und Orca (T3 dort offen aus S2),
  Super gehalten während der Ausgabe auf DRM, Einfügen mit Noctalia-Verlauf.
- **Prüfung:** `scripts/check.sh --only fmt,clippy,unit,wayland` während der Arbeit, volle Suite einmal am Ende.
- **Isolation, Budget, Bericht:** wie 6a (eine bis zwei Sitzungen, Halt nach zwei erfolglosen Ansätzen je
  Teilproblem, Bericht 5–12 Zeilen).

**Ergebnis W2 (20.11.2026, Branch `wayland-w2`):**

- Gebaut: Textausgabe in `dictate-ui`. Ohne Wayland und mit Unit-Tests in `src/ui/`: `output.rs` (Warteschlange,
  Bereinigen, Zerlegen in Zeilen und Tasten, Wege nach Entscheidungen 8 und 11, Terminal-Entscheidung, Einstellungen
  aus `get_config`), `keymap.rs` (Belegung von T2 nur auf den 48 Codes des Hauptblocks plus Leerzeichen, Return,
  Rücktaste, Shift, Ctrl auf ihren echten Codes; Abschnitte bei mehr als 48 verschiedenen Zeichen; Anschläge, die
  immer alles loslassen), `keys.rs`/`fake_keys.rs` um Modifikatoren erweitert (weiter nur `EVIOCGKEY`; Testdatei
  `0 mod`). Im Binary `src/bin/dictate-ui/`: `output.rs` (Ausgabe-Thread, eigene Wayland-Verbindung, Warten auf
  losgelassene Modifikatoren mit Hinweis nach 1,5 s und Abbruch nach 30 s), `wl.rs` (virtuelle Tastatur mit Belegung
  über memfd und Loslassen in `Drop`, Eingabemethode, Data-Control mit Schreib-Threads und Frist, Inhalte als
  `Secret`, das beim letzten Verweis überschrieben wird), `clip.rs` (Einfügen, Zurücklegen, Rückfall Tippen),
  `niri.rs` (`FocusedWindow` über `$NIRI_SOCKET` mit Frist 500 ms; `niri-ipc = "=26.4.0"`). `start` trägt die
  `app_id` (Profil). Config: `typing.output` (`paste`/`type`, Vorgabe `paste`), `typing.keyboard_only_app_ids`;
  `actions.terminal_classes` um Wayland-`app_id`s ergänzt (footclient, ghostty, GNOME Console/Ptyxis/Terminal,
  Konsole). Die Eingabemethode wird beim Zustand `finalizing` angelegt und zerstört, sobald die Warteschlange leer ist
  (spätestens nach 10 s ohne Auftrag und beim Beenden).
- Prüfung: Schritt `wayland` 94 Prüfungen grün (W1 42, dazu `tests/wayland/output.sh`): T3 im GTK-Feld samt
  Zeilenumbruch und 5000-Byte-Text in Stücken, T2 im GTK-Feld (Ausnahmeliste) exakt, nur `BackSpace` aus
  `final.keys`, Modifikator gehalten (nichts geschrieben bis zum Loslassen; physisch gehaltenes Alt plus virtuelle
  Taste: schlichtes Zeichen, kein Kürzel; Positivkontrolle physisches Mod+Q löst aus), Abbruch mitten im Tippen
  (Warteschlange leer, keine Wiederholung danach), spätes `final` nach Abbruch verworfen und das nächste getippt,
  `start` mit `app_id`, Einfügen mit 6 Typen samt unbekanntem Typ und 5 MB byte-genau zurückgelegt (Verlauf mit
  Hinweis lässt den Text aus), Passwort-Zwischenablage (nur der Hinweis gelesen, Besitzer behält sie, getippt),
  Alacritty (T3 plus Return in der richtigen Reihenfolge), xterm über XWayland (T2, UTF-8 exakt, Return), SIGTERM
  mitten im Tippen (keine Wiederholung, Thread endet geordnet), Fokuswechsel mitten im Tippen (gestoppt vor dem
  Zeilenumbruch, nichts danach in einem der Fenster), gieriger Verlauf beim Einfügen (Ziel bekommt trotzdem den Text),
  Auswahl wechselt während des Sicherns auf ein Passwort (nicht übernommen, getippt), Einfügen in ein
  XWayland-Programm (GTK über X11: Weg Einfügen von selbst, Abholung nach dem Ctrl+V bestätigt), nichts davon im Log.
  **Gegenprobe:** ohne Loslassen (jede Taste nur gedrückt, kein Loslassen in `Drop`) scheitern genau die Prüfungen
  „no key repeats“ nach Abbruch und nach SIGTERM (34 → 48 bzw. 291 → 314 Zeichen).
- Volle Suite am Ende (20.11., nach der Durchsicht): fmt, clippy, unit (1615 Tests), xclip, wayland (91, 49 s ohne
  Bau), xephyr, gui, speech (CER 0,0058), gpu grün, 29 min; danach nur noch `wayland` erweitert (94 grün). Dauer von
  W2: eine Sitzung, etwa 1 h 45 min (14:54 bis 16:40) einschließlich CUDA-Erstbau, Durchsicht und Suite (Schätzung:
  ein bis zwei Sitzungen); die Durchsicht (Subagent) brauchte 11 min und rund 190 000 Token.
- Unabhängige Durchsicht (Sicherheit, Richtigkeit): 9 Befunde (4 mittel, keiner hoch), alle umgesetzt: Auswahl, die
  während des Sicherns wechselt, wird nie übernommen (Zähler der `selection`, Prüfung nach jedem Typ und vor dem
  Anbieten); Rundreise vor dem `Ctrl+V` und Ruhe ab der **letzten** Abholung (eine frühe Abholung eines gierigen
  Verlaufs legte sonst zu früh zurück); nach einem Wayland-Fehler beantwortet der Ausgabe-Thread Aufträge als
  gescheitert und verbindet neu (die Warteschlange stand sonst still); Fokus (niri-Fenster-ID) wird vor jedem
  Zeilenumbruch, jeder Rücktaste und dem `Ctrl+V` erneut geprüft, bei T3 vor jedem Stück, ob das Feld noch aktiv
  ist; Zurücklegen auf jedem Weg nach dem Anbieten; Frist der Modifikatoren vor dem `Ctrl+V` bricht ab statt zu
  tippen; der Hinweis-Typ wird nie abgeschnitten (mehr als 256 Typen: tippen); gesicherte Bytes werden beim Wachsen
  überschrieben; nach einem gescheiterten `flush` lässt `Drop` auch alle ungesicherten Tasten los. Dazu: Fehler der
  niri-Antwort ohne Werte im Log (Fenstertitel), Meldungen an eigene Quellen werden im Leerlauf geleert.
- **Abweichungen vom Auftrag:**
  1. Abnahme „Einfügen samt Zurücklegen aller Typen mit Hinweis“ nach Entscheidung 10 geändert: mit Hinweis `secret`
     wird getippt und die Zwischenablage nicht angefasst (6c umformuliert).
  2. Eingabemethode wird schon beim Zustand `finalizing` angelegt (ihr `activate` liegt dann beim `final` vor) und
     zerstört, sobald nichts mehr auszugeben ist; Wächter nach 10 s ohne Auftrag. Nicht beim `start` (eine
     freihändige Aufnahme hielte den einzigen Platz am Sitz minutenlang).
  3. Fokusprüfung vor Return, Rücktaste und `Ctrl+V` (aus der Durchsicht, nicht im Auftrag); ändert sich der Fokus,
     endet die Ausgabe mit Hinweis.
  4. Warten auf Modifikatoren mit Hinweis nach 1,5 s und Abbruch nach 30 s (die Lua-Seite hatte die Frist von
     xdotool); bei unlesbarem Tastenzustand wird getippt, nie eingefügt.
  5. Bei unbekanntem Fokus Zeilenumbruch als Shift+Return (Chatfelder senden sonst), in bekannten Terminals Return.
  6. Chromium-Test „jede Sonderposition“ (Folge 1 aus S2) nicht im verschachtelten niri, sondern als Unit-Test der
     Belegung (`keymap::tests`); Chromium/Electron live.
- **Offen:**
  1. **Live-Test mit Daniel** (Installation nur mit Zustimmung: `cargo install --path . --locked --bin dictate-ui
     --target-dir target`, dann `dictate-ui.service` neu starten): Diktat in Firefox, Alacritty, Slack und Orca (T3
     dort offen aus S2), Super gehalten während der Ausgabe auf DRM, Einfügen in einem XWayland-Programm mit Noctalias
     Verlauf (Text darf nicht im Verlauf landen, alte Zwischenablage zurück), Passwort aus dem Passwortmanager in der
     Zwischenablage, Dauer der niri-Abfrage live (`journalctl --user -u dictate-ui | grep 'ms to decide'`;
     verschachtelt 26–313 ms).
  2. **Installierter Daemon ist älter:** er und das Einstellungsfenster kennen `typing.output` und
     `typing.keyboard_only_app_ids` nicht (`deny_unknown_fields`): diese Schlüssel erst nach Neuinstallation des
     Daemons in `config.toml` setzen; die neuen Wayland-Terminals in `actions.terminal_classes` kommen auch erst damit.
     `dictate-ui` nimmt ohne sie die Vorgaben.
  3. ~~Einstellungsfenster~~ (erledigt nach W2, s. u.).
  4. ~~`toggle` ohne `app_id`~~ (erledigt nach W2).
  5. ~~Schreib-Threads ohne Obergrenze~~ (erledigt nach W2).
  6. ~~xwayland-satellite~~ (gemessen nach W2, s. u.).

**Nach W2 (20.11., gleiche Sitzung, Daniel: „weitermachen mit den offenen Punkten“; `dictate-ui` vorher auf Daniels
Wunsch installiert, Live-Test holt er später nach):**

- **Reihenfolge Eingabemethode/Tastatur (Fehler, gefunden unter Last):** bei Load 11–17 (fremder `pytest`) kam im
  GTK-Feld „Zweite Zeile“ (Commit) **vor** dem zuvor gesendeten Shift+Return an: zwei Wege ohne feste Reihenfolge.
  Behoben: vor einem Wechsel des Wegs wartet die Ausgabe (höchstens 500 ms) auf die Bestätigung des Felds: nach einem
  Commit, bis der gemeldete Text vor dem Cursor (`surrounding_text`, nur die letzten 64 Zeichen, nie geloggt) mit dem
  Text endet; nach einer Taste auf ein neues `done` (GTK meldet nur die aktuelle Zeile und nach Shift+Return keinen
  geänderten Text, gemessen). In Terminals mit Eingabemethode gehen Text und Zeilenumbrüche am Stück als ein Commit
  (`\n` kommt als Return an, S2). Danach unter Load 17 alle Prüfungen grün, ohne eine einzige Zeitüberschreitung.
- Fristen nach den Messungen unter Last: niri-Abfrage 1,5 s statt 500 ms (verschachtelt bis 960 ms), Warten auf das
  erste `done` der Eingabemethode 500 ms ab dem Anlegen statt 250 ms (meist schon in der Zeit des Abschließens).
- `toggle` mit `app_id`: neue Anfrage `{"cmd":"toggle","app":…}` (`Request::Toggle`, Daemon schaltet wie bisher und
  wählt beim Start das Profil); ein älterer Daemon ignoriert `app`. Test im Schritt `wayland`.
- Höchstens 16 gleichzeitige Schreib-Threads für die zurückgelegte Zwischenablage, darüber leere Antwort.
- **xwayland-satellite gemessen:** spiegelt die Typenliste samt `x-kde-passwordManagerHint` nach X (`xclip -t
  TARGETS` zeigte den Hinweis) und holt ohne Anfrage eines X-Clients nichts ab; die frühe Abholung im Test kam vom
  GTK-Fenster über X11. Live läuft unter niri kein X-Verlauf (nur `xwayland-satellite`, Prozessliste).
- **Chromium im Schritt `wayland`:** T2 (Ausnahmeliste) mit einem Text, der alle 48 Zeichenplätze und einen zweiten
  Abschnitt füllt, samt `ß:,-.;!?/()=+*#<>|@€„“–`: jedes Zeichen kommt an (Folge 1 aus S2 erfüllt).
- **Einstellungsfenster:** das Abbild `Typing` kannte die neuen Schlüssel nicht und hätte sie beim Speichern still
  verworfen (unbekannte Schlüssel bleiben nur auf oberster Ebene erhalten). Jetzt Felder auf der Seite „General“
  („Without an input method (Wayland)“: Paste/Type; „Apps that never get the input method“), Test für die Rundreise;
  Hinweis bei „Terminal classes“ nennt `app_id`.
- Schritt `wayland`: 97 Prüfungen; dazu `fmt,clippy,unit,gui` grün.
- **Installiert:** `dictate-ui` aus `wayland-w2` (20.11., 17:07 auf Daniels Wunsch, 17:12 erneut mit den Korrekturen oben),
  `dictate-ui.service` neu gestartet. **18:14 auf Daniels Wunsch auch der Rest** (`cargo install --path . --locked`:
  `dictate`, `dictate-settings`, `dictate-palette`, `dictate-diag`, `dictate-ui` aus `wayland-w2`), `dictate.service`
  neu gestartet (vorher `idle`): die echte `config.toml` lädt, `get_config` liefert `typing.output`,
  `typing.keyboard_only_app_ids` und die Wayland-Terminals, `dictate-ui` hat sich nach 1 s neu verbunden. Der
  installierte Daemon stammt damit aus einem Branch, nicht aus `main` (Merge erst auf Daniels Wunsch).
- **Vorschlag W3** (frische Sitzung, Branch `wayland-w3` von `wayland-w2`): Textaktionen unter niri: PRIMARY über das
  vorhandene Data-Control-Gerät (`wl.rs`, `primary_selection`; nur PRIMARY lesen, CLIPBOARD nur für die zwei
  Ausnahmen, Test dazu), Panel als GTK-Fenster (`dictate-panel`) mit gestreamtem Markdown, Copy über eine eigene
  Quelle mit Hinweis, Replace über den Ausgabeweg aus W2 (Fokusprüfung gegen das beim Öffnen gemerkte Fenster),
  Sperre über `LockedHint`, `Lock` und `PrepareForSleep` (Befund 11). Vorher den Live-Test von W2.

- Messungen (verschachtelt, llvmpipe): niri schickt `activate`/`done` einer frisch angelegten Eingabemethode erst nach
  einer Rundreise des Clients (ohne Rundreise kam binnen 250 ms nichts); `im_settle` macht darum immer mindestens
  eine. Die Abfrage `FocusedWindow` dauerte im verschachtelten niri 26–313 ms (Software-Rendering, niri ausgelastet);
  live messen (`journalctl --user -u dictate-ui | grep 'ms to decide'`).

## 6d. Auftrag W3 (nächste Sitzung, frisch)

Stand vorher: W2 fertig und am 20.11. installiert (alle Binaries und `dictate.service` aus `wayland-w2`), Live-Test von W2
durch Daniel offen (Testplan unter „Ergebnis W2“, Offen 1). Meldet Daniel Befunde daraus, haben sie Vorrang vor W3.

- **Branch:** `wayland-w3` von `wayland-w2`, eigener Orca-Worktree (neuer Worktree: CUDA-Erstbau ca. 10 min, allein,
  `CARGO_BUILD_JOBS=4`). Nichts nach `main`; Merge von `wayland-w1..w3` nur auf Daniels Wunsch, pushen nur auf Wunsch.
- **Ziel:** Textaktionen unter niri (Abschnitte 3.5, 3.6; Regeln 1, 6, 7 in Abschnitt 4; Befunde 6, 7, 9, 10, 11 in 6b;
  Zeilen von `actions.lua`, `lock.lua`, `markdown.lua` in der Tabelle von 6b):
  1. **PRIMARY über Data-Control** im Prozess (`src/bin/dictate-ui/wl.rs` empfängt `primary_selection` schon und gibt
     das Angebot frei). Je Quelle eine eigene Funktion; CLIPBOARD lesen nur für die zwei bekannten Ausnahmen (Sichern
     beim Einfügen in `clip.rs`, Palette-Absicht `explain_clipboard`, die erst W5 portiert); ein Test am Quelltext
     wie in `ui/keys.rs` hält das fest. Fristen und Grenzen wie `actions::selection` (`read_primary`); der Daemon-Teil
     (`act_*`, Geheimnisprüfung, Sitzung, Vorschau) bleibt unverändert.
  2. **Panel `dictate-panel`** als schwebendes GTK-Fenster (Fensterregel steht schon in `niri/dictate-binds.kdl`):
     Sitzungsautomat aus `actions.lua` nach Rust ohne GTK in `src/ui/` (Zustände closed/grabbing/select/working/
     result/error mit `gen`/`run_gen`, `act`-Aufrufe seriell, Leerlauf 300 s, Totzeit 0,3 s und `stale`-Tasten nach
     Befund 9, Reihenfolge nach Befund 10). Gestreamte Antwort (`delta`/`note`/`diff`/`result`/`cancelled`/`error`)
     als Markdown: `markdown.lua` nach Rust (Nummerierung wie `src/actions/markdown.rs`, `markdown_test.lua` als
     Testvektoren), gezeichnet mit GTK/Pango. Design nach Entscheidung 9 (`ui::palette`, Schrift aus GTK, Ecken 12).
     Panelgröße: Empfehlung fest mit Scrollbereich (Resize-Shader wirkt nur bei Größe durch niri, S4); vorher Daniel
     fragen, wenn es anders sein soll.
  3. **Copy** in CLIPBOARD über eine eigene Data-Control-Quelle, die `dictate-ui` weiter bedient (S3). **Daniel vorher
     fragen:** mit Hinweis `x-kde-passwordManagerHint: secret` (Noctalias Verlauf lässt es aus) oder ohne (wie unter X,
     im Verlauf sichtbar)? Rückmeldung nur mit Zahlen („Copied N characters“, Befund 7).
  4. **Replace** über den Ausgabeweg aus W2 (`output.rs`, Warteschlange, Eingabemethode/Einfügen/Tastatur): nur
     `output=replace`, nie im Terminal, nur wenn das beim Öffnen gemerkte Fenster (niri-ID, `niri::focused`) wieder
     fokussiert ist; niri gibt den Fokus nach dem Schließen asynchron zurück, darauf warten (Befund 6); Fortschritt und
     Abbruch wie in W2.
  5. **Sperre** (3.6, Befund 11): logind über den System-Bus (D-Bus über `gio`, das mit GTK schon da ist; keine neue
     Crate ohne Grund): `LockedHint` der eigenen Sitzung (über `XDG_SESSION_ID` bzw. die Sitzung des Users, nie eine
     feste Nummer), Signal `Lock` der eigenen Sitzung und `PrepareForSleep(true)`; vor `GetSession` abonnieren. Dann
     `act_clear`/`diag_clear` und Panel schließen. Test gegen ein falsches logind auf einem eigenen Bus im
     Laufzeitordner des Tests (`fake-logind` aus `lock.lua` als Idee).
  6. **Kürzel** Mod+I in `niri/dictate-binds.kdl` (nur `spawn` mit argv, z. B. `dictate-ui actions`); Mod+Shift+I
     (Diagnose) erst W4. Die installierte `~/.config/niri/dictate-binds.kdl` ändert nur Daniel bzw. mit seiner
     Zustimmung (chezmoi).
  7. Markierung und Antworten nie loggen, nie in `history.rs`, keine Shell (Regel 7).
- **Abnahme:** Schritt `wayland` erweitert: Markierung in einem GTK-Feld im verschachtelten niri → Mod+I öffnet das
  Panel schwebend und fokussiert; Aktion gegen den Mock-Daemon mit gestreamten Deltas; Copy landet in CLIPBOARD
  (`nested-clip save`); Replace ersetzt im Zielfenster und verweigert bei Fokuswechsel und im Terminal; Sperre über das
  falsche logind schließt das Panel und löscht die Sitzung; nichts davon im Log. Unit-Tests für Automat, Markdown,
  Totzeit und Reihenfolge. Live mit Daniel: Panel unter niri (Shader, Übersicht, Alt+Tab), echtes PRIMARY, Replace,
  Sperre über Noctalia.
- **Prüfung:** `scripts/check.sh --only fmt,clippy,unit,wayland` während der Arbeit (bei Änderungen am Daemon auch
  `unit` ganz), volle Suite einmal am Ende.
- **Isolation, Budget, Bericht:** wie 6a. Umfang größer als W2: in Blöcke schneiden (z. B. a: PRIMARY, Automat,
  Markdown; b: Panel, Copy, Replace; c: Sperre), je Block committen; ist der Kontext nach einem Block groß, Stand hier
  festhalten und eine frische Sitzung vorschlagen. Unabhängige Durchsicht (Sicherheit: Zwischenablage, Markierung,
  D-Bus) vor dem Abschluss. Bericht 5–12 Zeilen.

**Stand W3 (20.11.2026, Branch `wayland-w3`), je Block:**

- **Block a (fertig):** `src/ui/markdown.rs` (Port von `markdown.lua`: Blöcke, Inline, Pango-Markup je Stück maskiert,
  Wortdiff-Ansicht; Fälle aus `markdown_test.lua`, Codeblöcke gegen `actions::markdown::code_blocks` gegengeprüft),
  `src/ui/panel.rs` (Sitzungsautomat rein und ohne Uhr: Eingaben tragen `now`, Folgen als `Effect`; `gen`/`run_gen`,
  Totzeit 0,3 s, `stale`-Tasten für beide Wiederholungsformen (nur Drücken wie GTK unter Wayland, Loslassen+Drücken
  binnen 50 ms wie X), Leerlauf 300 s, Wartezustand erst nach 150 ms, Knöpfe/Zeilen je Zustand als `items()`),
  PRIMARY in `src/bin/dictate-ui/primary.rs` über das Data-Control-Gerät des Ausgabe-Threads (`Cmd::ReadPrimary`,
  Grenzen und Kodierung wie `Xclip::read_primary`, Wechsel mitten im Lesen = Fehler, Hinweis `secret` = nicht lesen),
  Lesen eines Angebots nur noch in `clip::read_offer`; Test `sources_stay_apart` am Quelltext (CLIPBOARD nur `clip.rs`,
  PRIMARY nur `primary.rs`). Befehle mit Antwort werden auch ohne Wayland-Verbindung beantwortet (`refuse`).
- **Entscheidungen für Block b (Lesart von Claude, aus dem Bestand):** Fokus (niri-ID, `app_id`) wird im Hilfsthread
  gemerkt, bevor das Panel erscheint; danach ist das Panel fokussiert, Replace prüft darum „Fenster beim Öffnen bekannt“
  und der Ausgabe-Thread wartet nach dem Schließen begrenzt, bis niri den Fokus dorthin zurückgibt (unbekannt = kein
  Replace). Fokusverlust des Panels schließt es und löscht die Sitzung (Ersatz für „Klick daneben“, offen in 6b). Das
  zweite Mod+I kommt über den Steuer-Socket (`dictate-ui actions`), nicht als Taste. Quellen `question`/`clipboard`
  und `text_action` erst W5.
- **Block b (fertig):** Panel `src/bin/dictate-ui/panel.rs` (GTK-Fenster `dictate-panel` ohne Dekoration, nicht
  fokussierbare Bedienflächen, alle Tasten im Capture an den Automaten, Eingaben nur über den Kanal (keine
  Wiedereintritte), Markdown als Pango-Markup, Codeblöcke mit eigenem Copy, Wortdiff-Ansicht, Sekundentakt), Höhe über
  niri (`niri::own_window` nur mit `app_id` **und** eigener PID, `set_window_height`; wächst beim Streaming höchstens
  alle 150 ms, schrumpft nur beim Zustandswechsel außer zur Arbeit; höchstens 760 bzw. 85 % des Bildschirms), Daemon-Seite
  `src/bin/dictate-ui/act.rs` (serielle `act_*`-Warteschlange mit Überspringen veralteter Greifvorgänge, eigener Thread je
  `act_run` über `sock_client::Conn`, Copy ohne Hinweis über `Cmd::Copy`/`clip::copy`, das Kopierte bedient
  `dictate-ui` weiter und ein späteres Einfügen sichert es aus dem Speicher), Mod+I als `dictate-ui actions` (Steuer-
  Socket, Fokus im Hilfsthread gemerkt), Replace als eigener Auftrag der Warteschlange (`Job::replace`, wartet
  höchstens 1,5 s auf den Fokus des gemerkten Fensters, lehnt Terminals auch nach Zweck des Felds ab), KDL: Mod+I,
  Fensterregel ohne feste Höhe, mit `geometry-corner-radius 12` und `clip-to-geometry`. Beim Beenden mit offenem Panel
  `act_clear` sofort. Prüfung: `fmt,clippy,unit` grün (unit 590 s unter Last), `wayland` 133 grün (zuletzt nach den Korrekturen unten erneut) (neu
  `tests/wayland/actions.sh`: PRIMARY exakt mit `app_id`, Panel schwebend und fokussiert, Strom in 26 Stücken, Höhe
  454 → 760 über niri bei gleicher Breite, Copy byte-genau in CLIPBOARD ohne Hinweis, Replace im Zielfenster,
  verweigert ohne Zielfenster und in einem Fenster aus `terminal_classes`, Esc/zweites Mod+I/Fokusverlust schließen
  mit `act_clear`, Esc mitten in der Arbeit schickt `act_cancel` und bleibt offen, nichts davon im Log). Mock-Daemon
  kann jetzt mehrere Anfragen je Verbindung (wie der echte).
- **Abweichungen in Block b:** (1) Replace meldet nur Beginn und Ende im Overlay, keinen Fortschritt je Zeichen (W2 hat
  keinen Fortschrittskanal); (2) PRIMARY mit Hinweis `secret` wird nicht gelesen (Erweiterung nach Entscheidung 10);
  (3) Fokusverlust schließt (Ersatz für „Klick daneben“); (4) beim Start der Arbeit schrumpft das Panel nicht (sonst
  springt es 454 → 170 → wieder größer, gemessen); (5) die installierte `~/.config/niri/dictate-binds.kdl` ist
  unverändert: Mod+I und die neue Fensterregel wirken live erst, wenn Daniel die Datei aus dem Repo übernimmt.
- **Dauer a+b:** eine Sitzung, etwa 1 h 45 min (18:28 bis 20:12) einschließlich Erstbau unter Last 11–17 (Schätzung W3
  gesamt: eine bis zwei Sitzungen). Token nicht gemessen (kein Zähler in der Sitzung).
- **Offen für Block c (frische Sitzung, Branch `wayland-w3`):** Sperre (Punkt 5 oben: logind über `gio` auf dem
  System-Bus, `LockedHint` der eigenen Sitzung, `Lock` der eigenen Sitzung, `PrepareForSleep(true)`, vor `GetSession`
  abonnieren; dann `Panel::close` (löscht über `act_clear`) und `diag_clear`), Test gegen ein falsches logind auf eigenem
  Bus (Idee `fake-logind` aus `awesome/test`), dabei `DBUS_SYSTEM_BUS_ADDRESS` für **alle** Wayland-Tests auf einen
  privaten Bus setzen und ohne ihn abbrechen (wie die Prüfung von `XDG_RUNTIME_DIR`). Danach unabhängige Durchsicht
  (Sicherheit: Zwischenablage, Markierung, D-Bus, niri-Aktion), volle Suite einmal, Bericht. Dazu aus der Durchsicht
  des Beraters nach Block b:
  1. **Diktat bei fokussiertem Panel** ist seit dem Commit „W3 Block b, Korrekturen“ im Ausgabe-Thread verweigert (`app_id` `dictate-panel`:
     getippte Buchstaben wären Aktionstasten, `e` sendet an die KI); Test im Schritt `wayland` fehlt noch.
  2. Für die Durchsicht ausdrücklich nennen: Punkt 1; ein `Cmd::Copy`, das nach Ablauf der 10 s im Aufrufer doch noch
     ausgeführt wird (Rückmeldung „gescheitert“, Zwischenablage trotzdem gesetzt); Log-Zeilen mit Fehlertexten des
     Daemons (`grab failed: {e}`, `simple`) – dürfen keine Inhalte tragen.
  3. Wieder geöffnetes Panel nach einer 760 hohen Antwort: erscheint es erst in 760 und schrumpft dann? (GTK behält
     die letzte Größe; vor `present()` die Vorgabegröße zurücksetzen.)
  4. Mauspfad ohne Prüfung: Hover baut neu auf, der Klick trifft ein neues Element; je ein `nclick` auf eine Zeile und
     auf ✕ in `actions.sh`.
  5. Protokoll an einer Stelle lesen: das Lesen der Endantwort von `act_run` aus `act.rs` nach `ui::panel` ziehen und
     mit echten Antworten des Daemons testen (Mock und Client teilen sonst nur eine Lesart).
  6. Doku zum Abschluss: `CLAUDE.md` (Stand, Fallen: Entfernen eines abgelaufenen glib-Zeitgebers ist ein Panic,
     `set_prgname` setzt die `app_id`, niri-Aktion nur auf das eigene Fenster), Kopf von `check.sh` um `actions.sh`.
- **Block c (20.11., Branch `wayland-w3`):**
  - **Sperre (fertig):** reiner Teil `src/ui/lock.rs` (Kandidaten `XDG_SESSION_ID` → `auto`, Pfadprüfung, eigene
    Sitzung, Signale vor der Antwort auf `GetSession` gemerkt und danach geprüft, höchstens 32; `PrepareForSleep(true)`
    gilt sofort), gio-Teil `src/bin/dictate-ui/lock.rs` (System-Bus asynchron im Hauptthread, drei Abos nur mit
    Absender `org.freedesktop.login1` vor `GetSession`, `LockedHint` einmal beim Start gelesen, `exit-on-close` aus,
    jedes Scheitern als Warnung). Sperre: offenes Panel schließen (`Panel::close`, bricht eine laufende Aktion mit
    `act_cancel` ab und löscht mit `act_clear`), sonst `act_clear`; immer `diag_clear` (wie `on_lock` in Lua); ein
    Mod+I, dessen Fokus gerade gemerkt wird, öffnet nicht mehr. Test: `harness.sh` startet für **jeden** Wayland-Test
    einen privaten `dbus-daemon` (eigene Config ohne Aktivierung) als `DBUS_SYSTEM_BUS_ADDRESS` und entfernt
    `XDG_SESSION_ID`; `run.sh` bricht ohne ihn ab; `tests/wayland/lock.sh` gegen `awesome/test/fake-logind` auf
    diesem Bus (drei Auslöser bei offenem Panel, mitten in der Arbeit, bei geschlossenem Panel, Gegenproben fremde
    Sitzung/Entsperren/nach dem Schlaf, Rückfall auf `auto`, schon gesperrt beim Start): 25 Prüfungen grün, `wayland`
    gesamt 147 s. Live offen: ob der User-Dienst `XDG_SESSION_ID` hat und `auto` außerhalb einer Sitzung die
    Wayland-Sitzung liefert (im Log: `lock: watching logind session … (from "…")`), Sperre über Noctalia.
  - **Punkte 1, 3, 4 (fertig):** (1) `actions.sh` 21b: Diktat „e“ bei fokussiertem Panel wird verweigert, keine
    Aktion, Panel bleibt offen und fokussiert. (3) Wiederöffnen nach der 760 hohen Antwort, gemessen über niris
    Ereignisstrom (jede Größe): 456 → 454, nie 760; GTK behält die alte Größe also nicht, keine Änderung nötig, der Test
    bleibt als Wächter. (4) Mauspfad: der neue Test fand **zwei Fehler aus Block b**: das ✕ im Kopf war mit der Maus
    wirkungslos (`enabled("close")` kannte es nicht, nur Esc schloss), und Hover baute das Panel neu auf, sodass der
    Klick auf eine Zeile verloren ging (Drücken auf dem alten, Loslassen auf dem neuen Element). Behoben: ✕ gilt bei
    offenem Panel immer, Hover liefert `Effect::Highlight` (nur Klassen und der Hinweis der Auswahl, kein Neuaufbau),
    Unit-Tests dazu. Klickziele kommen aus dem Debug-Log `panel: layout <id> x y w h` (nur Kennungen und Zahlen) plus
    Lage des Fensters aus niri; Hover und Klick stehen mit Kennung im Debug-Log. `wayland` 167 grün.
  - **Punkt 5 (fertig):** die Endantwort von `act_run` liest `ui::panel::RunEv::from_reply` (Text und Diff
    herausgenommen, nicht kopiert); `act.rs` ruft nur noch sie. `tests/daemon_actions.rs`
    `panel_reads_the_real_replies` prüft sie und `GrabInfo::from_json` gegen den echten Daemon: Antwort, Ersetzen mit
    Wortdiff, Hinweis einer wachsenden Aktion, abgeschnitten, Fehler der KI, unbekannte Auswahl, Abbruch.
  - **Punkt 6 (fertig):** `CLAUDE.md` (Stand W3, Wayland-Tests mit privatem System-Bus, Fallen GTK/niri/Sperre), Kopf
    von `check.sh`.
  - **Unabhängige Durchsicht (Sicherheit, Opus, nur lesend, etwa 10 min):** 0 hoch, 2 mittel, 5 niedrig, alle
    umgesetzt. **M1:** Copy und PRIMARY-Lesen mit Frist liefen nach deren Ablauf doch noch (Copy „gescheitert“ und
    trotzdem in CLIPBOARD, auch nach Sperre) → Anspruchs-Slot (`output::Claim`: wer den Inhalt zuerst nimmt,
    entscheidet; Unit-Tests), `Job::Copy` prüft `gen` wie das Greifen (auch kurz vor dem Setzen), ein Greifen, das nach
    dem Schließen fertig wird, löscht selbst, die Sperre löscht auf eigenen Verbindungen (`Act::clear_all_now`) statt
    hinter einem bis 20 s wartenden Auftrag. **M2:** eine veraltete `XDG_SESSION_ID` hätte still die falsche Sitzung
    beobachtet → `XDG_SESSION_ID` und `auto` werden beide gefragt und beide beobachtet, Warnung bei Abweichung (Test mit
    Sitzung 3 im falschen logind). **N1:** Diktat ins Panel nur an niris `app_id` erkannt → zusätzlich `panel_visible`
    des Hauptthreads, geprüft beim Start, je Zeichen, je Stück und vor Ctrl+V (Test 26c: Panel öffnet mitten im
    Tippen, danach kein Zeichen). **N2:** Sperre bricht jetzt die Ausgabe ab (wie Mod+Ctrl+E), `Watch` führt den
    Zustand (`LockedHint` oder Einschlafen; ein `Lock`-Signal allein nicht, sonst bliebe er ohne Sperrprogramm
    stehen), Mod+I öffnet bei gesperrter Sitzung nicht (Test 31b). **N3:** Markdown aus KI-Antworten: Schachtelung
    höchstens 8, Suchen je Muster gemerkt (Ketten offener Marken linear; Test mit 100 000 `*` und anderen Ketten
    unter 0,5 s, gemessen zusammen 0,02 s). **N4:** Text eines Auftrags (Diktat, Replace) und seine Stücke werden beim
    Verwerfen überschrieben (`Drop` für `Job` und `Pieces`, `SecretText` für bereinigte Kopien). **N5:** Bus weg oder
    beim Start nicht da: neuer Versuch mit 2 s bis 60 s Abstand (späte Antworten einer alten Verbindung verworfen).
    Gegenprobe mit fremdem Absender (`gdbus emit`) in `lock.sh`. Nicht übernommen (Produktentscheidung Daniel): eine
    Zeile „Lock detection off“ im Panel, solange die Erkennung aus ist.
  - **Nachtrag:** der Quelltext-Test `sources_stay_apart` (Liste aller Module) war seit dem Sperr-Commit rot, weil dort
    nur die Bibliothekstests gezielt liefen; `lock.rs` steht jetzt in der Liste.
  - **Volle Suite (20.11., 21:38–22:13, HEAD 8c49633):** alle 9 Schritte grün in 2090 s (fmt 2, clippy 0, unit 723,
    xclip 10, wayland 152 mit 176 Prüfungen, xephyr 766, gui 91, speech 243, gpu 102) unter Last 13–15 (andere
    Sitzung). Logs `/home/data/stt-test/logs/check/20261120-213826`. Dauer Block c: etwa 1 h 45 min (20:28 bis 22:15)
    ohne die Suite; Token nicht gemessen.
  - **Nach der Suite (Hinweis des Beraters):** die Sperre von Mod+I hing an jeder beobachteten Sitzung; eine veraltete
    `XDG_SESSION_ID` mit hängendem `LockedHint` hätte Mod+I für immer gesperrt (auf blub ist die Wayland-Sitzung heute
    Nummer 2 mit `LockedHint=yes`, nur lesend über `loginctl` gesehen; ob gerade gesperrt, ist von hier nicht zu
    sehen). Jetzt sperrt nur die Sitzung hinter `auto` das Öffnen (ohne sie alle), gelöscht wird bei jeder; die
    Ablehnung erscheint als Hinweis im Overlay. Gegenproben mit fremdem Absender prüfen jetzt, dass `gdbus emit`
    gelang. Danach `fmt,clippy,wayland` grün (182 Prüfungen, 206 s) und `--bin dictate-ui` 211 grün; die volle Suite
    lief auf dem Stand davor (8c49633).
  - **Ohne eigenen Test (gelesen bzw. nur indirekt):** Neuverbindung zum System-Bus (N5); dass die Sperre ein
    laufendes Replace oder Diktat abbricht; `gen`-Prüfung von Copy, das Aufräumen nach einem späten Greifen und
    `clear_all_now` (`lock.sh` zählt nur „mindestens ein Löschen mehr“); dass `panel_visible` nicht hängen bleibt
    (gelesen: `Show` und `Hide` hängen beide an `visible`, jedes Schließen läuft über `close()`).
  - **Verhaltensänderungen über den Auftrag hinaus (für Daniel):** eine Sperre verwirft ein laufendes Diktat und
    eine laufende Ausgabe; Mod+I ist bei gesperrter Sitzung verweigert; das Tippen stoppt, sobald das Panel aufgeht;
    `awesome/test/fake-logind` kennt jetzt Sitzung 3; Lage-, Hover- und Klick-Zeilen im Debug-Log (nur Kennungen und
    Zahlen) sind ein Testhaken im Produktcode.
  - **Abschluss (20.11., 23:15):** volle Suite erneut grün auf `08c60f1` (1875 s, Last 5–12), Fast-Forward nach `main`
    und Push (`main` und `wayland-w3`), installiert aus `wayland-w3` (alle Binaries), `dictate.service` und
    `dictate-ui.service` neu gestartet. **Live gemessen:** der User-Dienst hat `XDG_SESSION_ID=2`, und `auto` liefert
    dieselbe Sitzung `_32` (keine Warnung). `niri/dictate-binds.kdl` über chezmoi übernommen (Branch
    `niri-dictate-binds` in der chezmoi-Quelle, gemergt nach `master`, nicht gepusht), vorher an einer Kopie mit
    `niri validate` geprüft; niri lud die Config fehlerfrei neu. Vorher (Stand bis hier):
  - **W3 damit fertig gebaut, nicht installiert.** Offen live mit Daniel: Panel unter niri (Shader, Übersicht, Alt+Tab),
    echtes PRIMARY, Replace, Sperre über Noctalia mit Blick ins Log (`lock: watching logind session …`, ob eine Warnung
    „sessions differ“ kommt; `loginctl show-session <id> -p LockedHint` jeder beobachteten Sitzung vor und nach dem
    Entsperren), Mod+I erst nach Übernahme von `niri/dictate-binds.kdl`; Installation aus `wayland-w3`
    nur mit Daniels Zustimmung. Produktfrage: Hinweis im Panel, wenn die Sperr-Erkennung aus ist.
  - **Vorschlag W4 (Diagnose-Panel):** dieselben Bausteine: Automat rein in `src/ui/` (Zustände `d_profiles … d_error`
    aus `diagnose.lua` mit `gen`, Totzeit, Fristen je Aufruf), `PanelUi` als gemeinsame Hülle (Kopf mit ✕, Zeilen,
    Knöpfe, `Effect::Highlight`, Lage-Log für Klicktests), Höhe über niri (`own_window`, `resizer`), Aufrufe an
    `dictate-diag` bzw. `diag_*` seriell wie `act.rs` mit Anspruchs-Slot, Sperre schließt auch dieses Panel, Mod+Shift+I
    in der KDL, Boot-Hinweis im Overlay; Test im verschachtelten niri gegen einen Mock mit `diag_*`.
- **Daniel, 20.11. (vor Block b):** (12) **Copy ohne Hinweis** `x-kde-passwordManagerHint` (bewusstes Kopieren landet im
  Noctalia-Verlauf wie unter X; `dictate-ui` bedient den Inhalt weiter, bis etwas anderes kopiert wird). (13) **Panel
  wächst über niri-IPC** (Höhe passend zum Inhalt bis zu einer Obergrenze, damit der Resize-Shader wirkt; eine
  schreibende niri-Aktion nur auf das eigene Fenster, erkannt an `app_id` `dictate-panel` und eigener PID).

## 6e. Auftrag W4 (nächste Sitzung, frisch)

> **Seit W7a gelöscht:** `awesome/` (auch `diagnose.lua`, `actions.lua`, `palette*.lua`) und die CLI-Clients
> (`src/diagnose/client.rs`, `src/palette/client.rs`, `src/client.rs`). Vorlagen für W4/W5 aus dem Stand vor W7:
> `git show 30cad69:<pfad>`; Lua-Hilfen für W5 liegen in `c552237:docs/w5-vorlagen/`. Die `skip`-Positionen eines Profils
> immer aus `diag_plan` nehmen (`last_crash` hat seit W7a 8 Einträge).

Stand vorher (21.11.): W1–W3 und W7 (Awesome und X11 entfernt, GUI-Tests im verschachtelten niri) gemergt, gepusht und
installiert (`main` `2e4061e`); der Claude-Schlüssel gilt wieder (CLAUDE.md, Offen 3). Live-Test von W2 und W3 durch
Daniel offen. Meldet Daniel Befunde daraus, haben sie Vorrang vor W4.

- **Branch:** `wayland-w4` von `main`, eigener Orca-Worktree (neuer Worktree: CUDA-Erstbau ca. 10 min, allein,
  `CARGO_BUILD_JOBS=4`). Pushen nur auf Wunsch, nach `main` erst nach grüner Prüfung und auf Daniels Wunsch.
- **Ziel:** Diagnose-Panel unter niri (Zeile `diagnose.lua` in 6b, Befunde 10 und 12; `PLAN-diagnose.md` für Ablauf
  und Sicherheitsregeln; CLAUDE.md „Diagnose-Bericht … nie loggen“). Auf denselben Bausteinen wie W3:
  1. **Automat rein in `src/ui/`** (neues Modul, z. B. `ui::diag_panel`, ohne GTK, Eingaben mit `now`, Folgen als
     `Effect`): Zustände aus `diagnose.lua` (`d_profiles`, `d_items`, `d_progress`, `d_preview`, `d_send`, `d_answer`,
     `d_error`; `set` 1271), `gen`/`run_gen`, Totzeit und `stale`-Tasten wie `ui::panel` (am besten gemeinsam
     herausziehen statt kopieren), Leerlauf, `split_report`, `mark_line`, `valid_spans`, `notes_items`,
     `parse_answer`, `unit_matches`, `preset_of`, `on_progress`, `show_error`, `back`/`stop`; Unit-Tests mit den
     Fällen aus Lua (Zeilennummern in 6b).
  2. **Aufrufe:** `diag_*` an den Daemon über `sock_client` (wie `act.rs`), seriell mit Fristen je Aufruf wie
     `TIMEOUTS` in `diagnose.lua` (collect/check/units 40 s, send 330 s), Fortschritt gestreamt; Befehle an den
     Ausgabe-Thread nur über den Anspruchs-Slot. Bericht, Probenausgaben und Antworten nie loggen, nie in argv, nur im
     Speicher der Sitzung, beim Schließen überschreiben; Klartext der Pseudonyme nie im Log, in Copy report oder in
     Aufrufen an den Daemon.
  3. **Fenster:** `PanelUi` aus W3 zu einer gemeinsamen Hülle machen (Kopf mit ✕, Zeilen, Knöpfe, `Effect::Highlight`,
     Lage-Log für Klicktests, Höhe über niri mit `own_window`/`resizer`), Inhalt je Zustand. **Entschieden (Daniel,
     21.11.): dasselbe Fenster `dictate-panel`** mit Quelle `diagnose`, ein Panel zur Zeit; Mod+Shift+I bei offenem
     Textaktionen-Panel schließt dieses (wie unter Awesome), die Fensterregel gilt für beide.
  4. **Sperre:** schließt auch dieses Panel, `diag_clear` (schon in W3), Mod+Shift+I bei gesperrter Sitzung verweigert.
  5. **Kürzel:** Mod+Shift+I in `niri/dictate-binds.kdl` (`spawn` mit argv, z. B. `dictate-ui diagnose`); die
     installierte Datei über chezmoi nur mit Daniels Zustimmung.
  6. **Boot-Hinweis** (`diag_boot_hint`, Befund 12: nach 10 s, alle 20 s, höchstens 4-mal) im Overlay von
     `dictate-ui`, Klick bzw. Kürzel öffnet das Panel.
  7. **Symptom des Users** (Entscheidung Daniel, 21.11.): optionales kurzes Textfeld „What is wrong?“ im Panel, als
     eigener, gekennzeichneter Teil der Anfrage an die KI (Daten, keine Anweisung; im Prompt als solcher benannt), durch
     den Geheimnisfilter, sichtbar in der Vorschau vor dem Senden, nie geloggt, nie in `history.rs`, nie in argv. Daemon:
     Feld in `diag_send` (bzw. dem passenden Aufruf), Grenze der Länge, Tests (wirkt nie als Anweisung, erscheint nie im
     Log, Schwärzung greift). Haiku schwächte Fall 7 der Messung ohne dieses Feld ab (`diagnose-ai.md`).
  8. pstore ist auf blub lesbar (ACL, CLAUDE.md Offen 8): die Probe `pstore` liefert dort jetzt Inhalt; im Panel und im
     Bericht wie jede andere Probe behandeln (Schwärzen, Grenze).
- **Abnahme:** Schritt `wayland` erweitert (`tests/wayland/diagnose.sh`): Mock-Daemon mit `diag_*` (Profile, Plan,
  Fortschritt, Vorschau mit Schwärzungen, Senden, Antwort mit Belegen, Fehler), Mod+Shift+I öffnet schwebend und
  fokussiert, Ablauf bis zur Antwort mit Tasten und Maus, Copy report ohne Klartext der Pseudonyme, Sperre schließt und
  löscht, Boot-Hinweis erscheint und öffnet; nichts davon im Log (Prüfung am Ende von `run.sh` um die Testtexte
  erweitern). Unit-Tests für Automat und Zerlegung. Live mit Daniel: echter Schlüssel, Panel unter niri.
- **Prüfung:** `fmt,clippy,wayland`, gezielt `cargo test --release --bin dictate-ui` und `--lib ui::…`, bei Änderungen
  am Daemon `unit`; volle Suite einmal am Ende. Unabhängige Durchsicht (Sicherheit: Bericht, Pseudonyme, Logs, Panel,
  Sperre). Bericht 5–12 Zeilen.
- **Schätzung:** eine bis zwei Sitzungen (Lua-Vorlage 2265 Zeilen, Bausteine aus W3 wiederverwendbar).

### Stand W4 (Branch `wayland-w4` von `main` `4052225`, Sitzung vom 21.11.2026)

Lesung festgehalten, damit die nächste Sitzung `diagnose.lua` nicht neu lesen muss (Vorlage:
`git show 30cad69:awesome/dictate/diagnose.lua`, Test `git show 30cad69:awesome/test/diagnose.sh`).

**Zuschnitt:** Block A = gemeinsame Eingabesperre aus `ui::panel` herausziehen (reiner Umbau), Symptom-Feld im Daemon
(Punkt 7, Daemon-Teil), Automat `ui::diag_panel` mit Tests (Punkt 1). Block B = Aufrufe (Punkt 2, `diag.rs` im Binary)
und Fenster (Punkt 3, gemeinsame Hülle). Block C = Sperre, Kürzel, Boot-Hinweis, pstore (Punkte 4–6, 8) und
`tests/wayland/diagnose.sh`.

**Protokoll (Antworten des Daemons, `src/diagnose/service.rs`):**

- `diag_plan` ohne Profil: `{profiles:[{name,label,description,needs_unit}]}`; mit Profil (`unit`/`container` als
  Suchbegriff): `{items:[{index,label,probe:{probe,…},selected,network}]}` (`plan_json`).
- `diag_units`: `{units:[{unit,user}]}` (unter der Sperre der Sammlungen, Frist 40 s).
- `diag_collect` und `diag_check`: Zwischenzeilen `{"progress":{id,phase:"start",index,total,probe,params}}` bzw.
  `{…phase:"end",status,message,bytes,ms}`, dann die Vorschau. `params` und `message` können Namen tragen: nie loggen.
- Vorschau (`preview_json`): `{id,preview,profile,rounds,max_rounds,text,bytes,budget,sections:[{id,probe,params,status,
  included,lines,bytes,total_lines,cut,redactions:{art:n},placeholders:{präfix:n},header,round}],redactions,placeholders,
  pseudonymize,target:{host,model,cloud,ready,problem?},expires_in_s}`.
- `diag_send`: `{id,preview,rounds,max_rounds,model,answer:{findings:[{cause,confidence,evidence:[{section,from,to}]}],
  checks:[{probe:{…},label?,reason,network}],notes},restore:{findings:[[{start,end,placeholder,value}]],notes:[…]},
  dropped}`; `value` ist Klartext: nur im Speicher, beim Verwerfen überschreiben, nie zurück an den Daemon.
- `diag_copy`: `{text,bytes,preview}` (Text in CLIPBOARD, nur Zahlen ins Log). `diag_boot_hint`: `{show,reason}`.
- Fehler: `DISABLED` („Diagnosis is off…“), `UNKNOWN_SESSION` („Unknown or expired…“), „AI server changed…“,
  „The report changed since the preview…“; `cancelled:true` bei Abbruch.

**Lua → Rust (`ui::diag_panel`, rein):** Zustände `Profiles, Items, Progress, Preview, Send, Answer, Error` (+ `Closed`,
`Loading`); `gen` je Öffnung, jede Antwort trägt ihn. `start_profiles` (1326), `load_plan` (1361), `pick_profile` mit
`preset` (1427, `unit_matches` 1407, `preset_of` 1913), `pick_unit`/`toggle_item` (1487/1501), `collect`
(`skip` = nicht gewählte `index`, 1607), `on_progress` (1512), `start_progress`/neue Abschnitte nach Prüfrunde (1549, 1581:
`round_of` 448, `d.sec` auf den ersten neuen), `stop` (1635: Aufruf auflegen, `diag_cancel` mit bekannter id, zurück je
Runde), `toggle_section` (1654: ganze Abwahlliste als `deselected`), `move_section` (1691), `copy` (1709), `parse_answer`
(1735) mit `valid_spans` (947), `send` (1774, `reviewing` 783: dieselbe Vorschau nicht zweimal), `toggle_check`/
`run_checks` (1847/1856, `network` nur, wenn ein gewählter Prüfbefehl es braucht), `jump` (1878), `back` (1890),
`show_error` (1292, Sonderfälle Daemon fehlt, aus, abgelaufen, Server geändert), `answer_move` (1988: erst Belege, dann
Prüfbefehle, wenn noch Runden), `on_key` (2036, Tasten je Zustand), `split_report` (361), `mark_line` (395, Platzhalter
`<art>-N` nur bis zur Zahl je Art), `notes_items` (1006: in Code und Links bleibt der Platzhalter). Fristen je Aufruf
`TIMEOUTS` (collect/check/units 40 s, send 330 s, sonst 15 s). Kürzel unter Awesome: Mod+Shift+I zu → Diagnose auf, offen
(jede Quelle) → zu; Mod+I bei offener Diagnose → zu.

**Boot-Hinweis unter Awesome** (`init.lua` 1495–1511): Frage nach 10 s, bei Daemon fehlt oder `busy` bis zu 4-mal im
Abstand von 20 s; „ja“ zeigt 20 s lang „Last boot ended uncleanly · Mod+Shift+I to diagnose“ (nicht anklickbar).

**Entscheidungen Daniel (21.11., während W4):** (1) Das Feld „What is wrong?“ steht in der **Vorschau** über dem
Bericht, editierbar bis zum Senden (Tab oder Klick hinein, Enter übernimmt, die neue Vorschau zeigt den gefilterten
Text; Esc verlässt das Feld ohne Übernahme). (2) Boot-Hinweis: **Klick und Kürzel** öffnen das Panel; solange der
Hinweis steht, bekommt das Overlay eine Eingaberegion in seiner Fläche.

**Stand Block A:** gemeinsame Sperre `ui::gate` (`bd7afed`, reiner Umbau, Tests unverändert grün). Symptom im Daemon
(`aefffb5`): `diag_preview` mit `symptom` (leer = keins), bereinigt zu einer Zeile ohne Steuer-, Format- und private
Zeichen (`report::clean_symptom`, höchstens 300 Zeichen), gefiltert mit `filter_line` (Geheimnisse, Platzhalter) und
nach jeder Runde neu gefiltert, als Zeile `SYMPTOM_LABEL …` am Ende des Kopfs (Vorschau = Copy = Gesendetes), Vorschau
zählt hoch, `preview_json` trägt `symptom {text,redactions,placeholders}` und `max_symptom_chars`; der System-Prompt
nennt die Zeile nur, wenn es sie gibt, als Daten (nicht befolgen, nicht beantworten, kein Beleg). Tests: Bericht (Filter,
keine vorgetäuschte Kopfzeile, `residue` leer, Länge, Neufiltern nach gelerntem Namen), Prompt, Daemon (Vorschau, Copy,
Senden, Fehler bei Überlänge), Log (`markersymptom` nie im Log). Offen: ob das Feld Fall 7 der Messung mit Haiku
verbessert, ist eine eigene Messung mit Budget (nicht belegt).

Automat `ui::diag_panel` (`84424eb`, Tests in `src/ui/diag_panel_tests.rs`, 24 Fälle): Ansichten `Loading, Profiles,
Items, Progress, Preview, Send, Answer, Error`; jeder Aufruf hat `gen` und eine eigene `seq`, Antworten nur für einen
wartenden Aufruf (`Pending`); `Effect::{Call, Hangup, Show, Hide, Render, Highlight, Scroll, Field, Notice}`; `Call` mit
`request()`, `name()` (fürs Log) und `timeout()` (40 s Sammeln/Prüfen/Units, 330 s Senden, sonst 15 s); `CallErr` mit
`kind()` (fürs Log, nie der Text). Belege sind Elemente (`ev:<befund>:<beleg>`) wie Prüfbefehle (`check:i`); Klick auf
einen Prüfbefehl wählt, Enter führt aus. Abschnitte der Vorschau als `sec:S<n>` (Klick schaltet um). Feld `symptom`
(Tab/Klick hinein, Enter oder Tab übernimmt, Esc verwirft, Klick daneben übernimmt nur; nach neuem Sammeln wird das
übernommene Symptom selbst wieder gesetzt). Reine Hilfen für Block B: `split_report`, `line_number`, `mark_line` (Stücke
`Plain/Redacted/Placeholder`), `limits`, `valid_spans`, `parse_answer`, `params_of`, `Evidence::text`, `wipe_json`.
Dauer Block A samt Lesen rund 2,5 h (Schätzung 6e: eine bis zwei Sitzungen für W4 gesamt); Token nicht gemessen, der
Kontext war am Ende groß (Lesung von rund 7000 Zeilen), darum Übergabe statt Weitermachen.

**Prüfung Block A:** `fmt,clippy`, `--lib ui::` (105), `--bin dictate-ui` (234), `unit` (542 s) und `wayland` (230 s,
Last 15; prüft den Umbau `ui::gate` über `tests/wayland/actions.sh`) grün. Nachtrag `4bc25e2`: `Debug` der Inhaltstypen
des Automaten zeigt nur den Typnamen (`opaque_debug!`, Test `debug_output_never_shows_contents`), ein wartendes Symptom
wird beim Schließen überschrieben. Nicht gelaufen: volle Suite, unabhängige Durchsicht. Nicht gemessen: ob das Symptom
gegen ein echtes Modell nie als Anweisung wirkt (nur strukturell: Kennzeichnung, Lage, Prompt, Filter) und Fall 7 mit
Haiku (`live_eval.rs` setzt `symptom: false`; eine Messung braucht einen Fall mit Symptom).

**Auftrag Block B (nächste frische Sitzung, Branch `wayland-w4`):**

0. **Zuerst das Auflegen belegen** (Stop und Schließen im Automaten bauen darauf): Test neben
   `hanging_up_cancels_the_collection_and_leaves_nothing` (`tests/daemon_diagnose.rs`): `shutdown(Both)` auf einem
   Klon des Sockets, während ein anderer Griff blockiert liest; der Leser kehrt zurück, der Daemon bricht ab.
   Entscheiden, ob `Hangup` jeden laufenden Aufruf treffen soll: heute nur Sammeln, Prüfen und Senden; ein laufendes
   `diag_units` (bis 40 s) hält bei Back oder Schließen die Reihe auf (wie unter Lua).

1. **Aufrufe** `src/bin/dictate-ui/diag.rs` (neu; in die Liste von `sources_stay_apart` in `primary.rs` eintragen): ein
   Thread, Aufrufe der Reihe nach aus einem Kanal; `Effect::Call` mit veraltetem `gen` überspringen, **außer `Cancel`
   und `Clear`** (kommen beim Schließen mit altem `gen`). Je Aufruf `Conn::open`, `send(call.request().to_line())`, die
   gesendete Zeile überschreiben (`wipe`, das Symptom steht darin), `reply_with(now + call.timeout())`; Zwischenzeilen
   `{"progress":…}` als Nachricht an den Hauptthread (`DiagPanel::progress_line`). Abbruch (`Effect::Hangup { seq }`):
   vor dem Lesen einen Klon des Sockets (`try_clone`) unter `seq` ablegen, Hangup ruft `shutdown(Both)` darauf; der
   blockierte Leser bekommt `Closed`, gibt die Verbindung frei, der Daemon bricht beim Auflegen ab (`HangupWatch`).
   `Copy`: `diag_copy`, Text aus der Antwort nehmen, über `OutTx::copy` (wie `act.rs`) nach CLIPBOARD, an den
   Hauptthread nur `{"ok":true,"chars":n}`; vor dem Setzen prüfen, ob `gen` noch gilt. Antworten als `Value`
   (`std::mem::take(&mut r.v)`) an den Hauptthread, dort nach `DiagPanel::reply` mit `sock_client::wipe_value`
   überschreiben. Fehler: `Ok(None)` → `CallErr::DaemonDown`, `ReadErr::Timeout` → `Timeout`, `ok:false` →
   `CallErr::from_reply`. Log nur `call.name()`, Dauer, `CallErr::kind()`, nie Meldungen, Parameter oder Texte.
2. **Fenster** (gemeinsame Hülle, Entscheidung: dasselbe Fenster `dictate-panel`): `PanelUi` bekommt eine Quelle
   (`actions`/`diagnose`); Kopf, ✕, Knöpfe, `clickable`, `highlight`, `log_layout`, `fit`/`resizer` bleiben gemeinsam,
   `render` je Quelle. `App` hält `panel` (Textaktionen) und `diag` (`DiagPanel`); höchstens eins offen. Ansichten nach
   `render_*` in `diagnose.lua` (Zeilen 579–1235): Profile (Ziffer-Chips), Einträge (Kästchen, `params_of` als
   Unterzeile, „network“), Fortschritt (Zeilen je Probe mit Status, Bytes, ms; Sekundentakt), Vorschau (Mono, Kopf
   je Abschnitt mit ■/□, Zeilennummer grau, `mark_line`: Schwärzung rot hinterlegt, Platzhalter teal; Beleg-Zeilen
   hinterlegt; neue Abschnitte mit Balken links; Ziel-Server und Modell, Zusammenfassung „n of m sections · KiB …“,
   Legende), Senden (warten, Sekunden), Antwort (Befunde mit Sicherheit, Belege als Zeilen, Notizen als Markdown mit
   festem Warnsatz darüber und **ohne** Copy-Knopf für Codeblöcke; `p` zeigt Klartext nur in Fließtext, in Code und
   Links bleibt immer der Platzhalter: `notes_items` in Lua 1006, mit Test), Fehler. `ScrollTo` nach dem Zeichnen
   anwenden (`take_scroll`). Breite der Diagnose: Lua 800 (Fensterregel ist 720; mit Daniel klären oder 720 lassen).
   **Eingabefeld „What is wrong?“:** ein `gtk::Entry` (Länge `max_symptom_chars`), das über Neuaufbauten hinweg
   **dasselbe Widget** bleibt (sonst verliert es Fokus und Cursor); solange `field.editing`, lässt der Tasten-Controller
   im Capture alle Tasten außer Esc, Return, KP_Enter, Tab, ISO_Left_Tab an das Feld durch; `changed` → `field_text`
   (als `SecretText` im `UiEv`, nie in `Debug`), Fokus hinein per Klick → `field_focused`, `Effect::Field(bool)` setzt
   bzw. nimmt den Fokus. Auswahl im Feld setzt PRIMARY (GTK); hinnehmen, im Modulkopf vermerken.

**Auftrag Block C:** (Sperre schließt die Diagnose, Verweigern bei Sperre, Steuerbefehl `diagnose`, `dictate-ui
diagnose`, gegenseitiges Schließen mit Mod+I: **in Rest Block B gebaut**, siehe dort; in Block C nur noch testen.)
Sperre (schließt auch das Diagnose-Panel, `diag_clear` ist schon in `locked`; Mod+Shift+I bei
gesperrter Sitzung verweigert mit Hinweis), Steuerbefehl `diagnose` in `ui::ctl` und `dictate-ui diagnose`
(Mod+Shift+I bei offenem Textaktionen-Panel schließt es, Mod+I bei offener Diagnose schließt sie), Kürzel in
`niri/dictate-binds.kdl` (`spawn "dictate-ui" "diagnose"`; danach auch Schritt `gui` und `niri validate`),
Boot-Hinweis (Frage nach 10 s, bei Daemon fehlt oder `busy` bis zu 4-mal alle 20 s, „ja“ zeigt 20 s lang
„Last boot ended uncleanly · Mod+Shift+I to diagnose“; Entscheidung Daniel: **Klick und Kürzel** öffnen, solange der
Hinweis steht, bekommt das Overlay eine Eingaberegion in seiner Fläche, danach wieder leer), pstore wie jede Probe
(nichts zu bauen, im Test mit Fixture), `tests/wayland/diagnose.sh` und `mock_daemon.py` um `diag_*` (Abnahme in 6e),
Prüfung am Ende von `run.sh` um die Testtexte (auch das Symptom) erweitern. Danach volle Suite, unabhängige Durchsicht
(Sicherheit), Doku (CLAUDE.md Stand/Offen 8, PLAN-diagnose.md).

**Entscheidungen von Claude (Lesart, zur Bestätigung):** Abbruch eines laufenden Aufrufs durch Auflegen der Verbindung
(Klon des Sockets, `shutdown`), wie unter Awesome das Beenden des CLI; der Daemon bricht beim Auflegen ab. Kennungen im
Debug-Log nur über Indizes (`unit:3`, `check:2`, `section:4`), nie Namen. Symptom: siehe Block A.

**Stand Block B (Sitzung vom 21.11.2026, Branch `wayland-w4`):**

- **Punkt 0 (Auflegen belegt):** Test `shutdown_on_a_clone_hangs_up_a_blocked_call` (`tests/daemon_diagnose.rs`): ein
  Thread liest blockierend, ein anderer ruft `shutdown(Both)` auf einem Klon; der Leser kehrt mit Ende zurück (unter
  1 s), obwohl beide Griffe offen bleiben, sieht der Daemon das Auflegen (POLLHUP), beendet die hängende Probe und gibt
  die Sperre frei; für `diag_collect` und `diag_units`. **Entscheidung (Claude):** `close()` legt jeden wartenden Aufruf
  auf (außer `diag_cancel`/`diag_clear`), Back aus den Einträgen ein wartendes `diag_units`/`diag_plan` (Eintrag in
  `pending` entfernt, späte Antwort zählt nicht). Grund: die Aufrufe laufen der Reihe nach, ein laufendes `diag_units`
  (bis 40 s) hielte sonst `diag_cancel` und `diag_clear` auf; der Daemon beobachtet `diag_units` schon (`watched()`).
  Test `back_and_close_hang_up_a_running_unit_search`.
- **Punkt 1 (Aufrufe, `src/bin/dictate-ui/diag.rs`):** ein Thread, Aufrufe der Reihe nach; veraltetes `gen` wird
  übersprungen (ohne Antwort), außer `diag_cancel`/`diag_clear`. Auflegen geht **nicht** durch die Reihe: der Hauptthread
  ruft `Diag::hangup(seq)` direkt auf ein Register (laufender Aufruf mit Klon des Sockets, `Conn::hangup_handle`);
  läuft er, `shutdown(Both)`, hat er noch nicht begonnen, wird er gemerkt und übersprungen, ist er fertig, geschieht
  nichts (`started`: Aufrufe kommen aufsteigend, das Register wächst nicht). Der Aufruf steht ab seinem Beginn im
  Register (Klon erst nach dem Öffnen nachgetragen): ein Auflegen, während die Verbindung aufgeht, setzt
  `current_hung`, und der Aufruf sendet nichts (Fund der Durchsicht vor der Übergabe: vorher ging es dort verloren;
  Test `a_hangup_right_after_the_call_is_never_lost`, 300 Durchläufe, scheiterte vorher). Ausgetragen wird auf jedem Ausgang
  (Guard `Registered`), sonst bliebe der Klon offen und der Daemon sähe das Auflegen nie. Werte reisen als `Wiped`
  (überschreibt beim Freigeben), auch Zwischenzeilen; `Conn::reply_with` gibt `&mut Value` und überschreibt den Rest
  jeder Zwischenzeile (vorher blieb sie stehen). Fehler: kein Daemon und `Closed` ohne Auflegen → `DaemonDown`,
  Frist → `Timeout`, aufgelegt → `Cancelled`, `ok:false` → `CallErr::from_reply`. Copy: `diag_copy`, Text über
  `Clip` (im Binary `OutTx::copy`) nach CLIPBOARD, vorher `gen` geprüft, zurück nur `{"ok":true,"chars":n}`. Log nur
  Name, Dauer, Zahl der Zwischenzeilen, `CallErr::kind()`. In `app.rs`: `Msg::Diag`, `apply_diag` (Aufrufe, Auflegen,
  Hinweise; Fenster-Folgen bis Punkt 2 nur im Debug-Log), Zeitgeber `schedule_diag`, `diag_clear` beim Beenden mit
  offener Diagnose. Tests in `diag.rs` gegen einen Fake-Daemon (5: veraltet übersprungen, Cancel/Clear trotzdem;
  Fortschritt und Antwort; Auflegen eines blockierten Aufrufs, der Fake sieht es; Auflegen vor dem Start; Copy nur
  Zahl, nach Schließen nichts kopiert). `sources_stay_apart` kennt `diag.rs`.
- **Punkt 2, Teil 1 (Hülle):** `PanelUi` ist quellenneutral: `Source {Actions, Diagnose}` (Fenstertitel, `width()`;
  beide 720, die Diagnose hatte unter Awesome 800: **offene Frage an Daniel**), `shown: Option<(Source, Ansicht)>` statt
  `phase`, `begin(source, view) -> changed` (leert Inhalt, Knöpfe, Flächen; ✕ bleibt), `finish(active, changed,
  shrink)` (Hervorhebung, nach oben beim Wechsel, `fit`, `log_layout`), `set_title`, `highlight_active(active)` (die
  Textaktionen setzen davor ihren Hinweis), `button(&Item)` und die Helfer `clickable/label/markup/clear/hbox/vbox/chip`
  sind `pub(crate)`. `show(Source)`. In `apply_diag` wirken schon `Show` (Fenster anlegen, `set_panel_visible(true)`),
  `Hide`, `Highlight` (`diag.active`), `Scroll`; `Render` und `Field` stehen nur im Debug-Log. Textaktionen
  unverändert (Schritt `wayland` grün).
- **Prüfung bisher:** `fmt,clippy`, `--lib ui::` 106, `--bin dictate-ui` 242, `daemon_diagnose` (Auflegetests),
  `wayland` 245 s bei Last 11–13. Nicht gelaufen: `unit` ganz, `gui`, volle Suite. Dauer Punkt 0, 1 und Hülle samt
  Lesen rund 1,5 h; übergeben wegen des Kontexts (Lesung von `app.rs`, `panel.rs`, Automat, Lua-Ansichten).

**Rest Block B (nächste frische Sitzung, vor Block C):** Lua-Vorlage gezielt: `git show 30cad69:awesome/dictate/diagnose.lua`
(Zeilen: `preview_items` 463, `apply_scroll` 549, `render_profiles` 579, `render_items` 630, `STATUS_TEXT`/
`render_progress` 714/726, `render_preview` 787, `render_sending` 874, `mark_ph` 982, `cause_markup` 993,
`notes_items` 1006, `answer_items` 1041, `render_answer` 1100, `render_error` 1203).

1. **`notes_items` rein in `src/ui/`** (z. B. `ui::diag_panel` oder neues `ui::diag_notes`) mit Test: Spannen werden
   Marken (Lua `PH_OPEN i PH_CLOSE`, verworfen, wenn die Notizen die Marke schon enthalten), dann `markdown::parse`;
   Klartext nur in Fließtext, in Code, Codeblöcken und Links immer der Platzhalter; kein Copy-Knopf für Codeblöcke;
   fester Warnsatz darüber („Suggestions from the AI, written from the report. Check every command before you run it;
   dictate runs none of them.“).
2. **Ansichten** in einem neuen Modul `src/bin/dictate-ui/diag_view.rs` (in `MODULES` von `primary.rs` eintragen),
   `impl PanelUi { fn render_diag(&mut self, d: &mut DiagPanel) }` über `begin(Source::Diagnose, format!("{:?}",
   d.view))`/`finish(d.active, …)`; was es von `PanelUi` braucht (Body, Footer, Scroller, `items`, `tx`, Palette),
   als `pub(crate)` zugänglich machen. Knöpfe aus `DiagPanel::items()` (Typ `ui::panel::Item`, `button` passt),
   Zeilen per `clickable` mit den Kennungen des Automaten (`profile:N`, `unit:N`, `item:N`, `check:N`, `ev:F:E`,
   `sec:S<n>`, `symptom`). Texte wie Lua (siehe Zeilen). Jedes Stück aus Bericht und Antwort durch `markdown::esc`
   (sonst scheitert `set_markup` und die Vorschau zeigt nicht, was gesendet wird). Kein Widget je Berichtszeile: je
   Abschnitt ein Kopf (klickbar `sec:S<n>`, ■/□, Balken links für aktiv bzw. neu) und ein Mono-Label für den Körper
   (Zeilennummer grau, `mark_line`: Schwärzung rot hinterlegt, Platzhalter teal, Beleg-Zeilen hinterlegt); mit vollem
   Budget (48 KiB) die Zeit des Neuaufbaus messen. `take_scroll` nach dem Layout anwenden (Tick-Callback wie
   `log_layout`, Lage des Abschnitts bzw. der Zeile), Fortschritt und Senden mit Sekundentakt (`UiEv::Tick`).
3. **Verdrahtung in `app.rs`:** `Render` → `render_diag`; `UiEv` an das offene Panel leiten (`a.diag.is_open()` →
   `key_press`/`key_release`/`activate`/`hover`/`focus_lost`/`close`, Tick); `Effect::Field`.
4. **Eingabefeld „What is wrong?“:** ein `gtk::Entry` (Länge `max_symptom_chars`), **fester Platz außerhalb des neu
   gebauten Bereichs** (zwischen Kopf und Scrollbereich, nur in `Preview` sichtbar), damit es Fokus und Cursor über
   Neuaufbauten behält. `changed` → `UiEv::FieldText(SecretText)` (nie in `Debug`), Handler bei jedem
   programmatischen `set_text` blockieren (Esc, neue Vorschau, Schließen; `field_text` setzte sonst `editing`).
   Tasten durchlassen synchron am Fokus des Entry festmachen (der Capture-Controller gibt sonst immer `Stop`): hat das
   Entry den Fokus, gehen alle Tasten außer Esc, Return, KP_Enter, Tab, ISO_Left_Tab an das Feld. Fokus per Klick →
   `field_focused`; `Effect::Field(bool)` setzt bzw. nimmt den Fokus. Beim Verbergen den Text löschen. Auswahl im Feld
   setzt PRIMARY (GTK): hinnehmen, im Modulkopf vermerken.
5. **Öffnen vorziehen (Abweichung von Block C, damit die Ansichten sichtbar werden):** `UiCmd::Diagnose`, `dictate-ui
   diagnose`, in `app.rs` öffnen (`diag.toggle`), bei Sperre verweigern mit Hinweis, `locked()` ruft auch
   `diag.close()`; ein offenes Textaktionen-Panel schließt dabei (ein Panel zur Zeit). Kürzel in der KDL bleiben Block C.
   Rauchtest im verschachtelten niri mit Mock-Daemon (`diag_*`) und Screenshots je Ansicht; `tests/wayland/diagnose.sh`
   bleibt Block C.
6. Prüfen: `fmt,clippy,wayland`, `--bin dictate-ui`, `--lib ui::`; Stand hier nachtragen.

**Stand Rest Block B (Sitzung vom 21.11.2026, Branch `wayland-w4`, Commits `7002456`, `62a6a55`, `a4316e0`):**

- **Punkt 1 (`ui::diag_notes`, `7002456`):** `notes_items` zerlegt die Notizen mit Marken (U+E000 i U+E001), dann
  `markdown::parse`; anders als Lua wird erst an den Marken zerlegt, dann jedes Stück einzeln maskiert. Klartext nur in
  Fließtext (Absatz, Überschrift, Zitat, Listenpunkt, auch die Beschriftung eines Links), in Inline-Code, Codeblöcken
  und Linkzielen immer der Platzhalter; Marken in den Notizen selbst → alle Spannen entfallen; eine Marke ohne gültige
  Spanne erscheint nie. Dazu `cause_markup`, `NOTES_WARNING`, `Counts` (Werte, Platzhalter, geschützt; nur Zahlen ins
  Debug-Log). 6 Tests. `Notes` überschreibt beim Freigeben Blöcke und Werte.
- **Punkt 2 (`src/bin/dictate-ui/diag_view.rs`, in `MODULES` von `primary.rs`):** `PanelUi::render_diag` mit allen
  Ansichten nach Lua (Profile mit Ziffer-Chips, Einträge und Units mit Kästchen, Fortschritt mit Punkt, Status, Bytes,
  ms und Sekundentakt, Vorschau, Senden, Antwort mit Befunden, Belegen, Notizen ohne Copy-Knopf und Prüfbefehlen,
  Fehler). Vorschau: je Abschnitt ein klickbarer Kopf (`sec:S<n>`, ■/□, Balken für gewählt bzw. neu) und **ein**
  Mono-Label für den Körper; Sprung zu Abschnitt bzw. Zeile nach dem Layout (Tick-Callback, Lage der Zeile aus dem
  Pango-Layout, 36 px Luft). Jedes Stück durch `markdown::esc`; `insert_hyphens="false"` in Bericht und Codeblöcken
  (Pango setzte beim Zeichenumbruch sonst Trennstriche, die nicht im Bericht stehen: Fund im Rauchtest).
  **Messung (48 KiB, Last 3):** der Aufbau selbst 2–9 ms, das Messen des großen Labels (`fit`) rund 630–660 ms je neuem
  Bericht (Sammeln, Abwählen, Symptom) und bei jedem Wechsel in die Vorschau (Sprung zu einem Beleg, zurück aus der
  Antwort: der Ansichtswechsel verwirft den Bericht). Darum bleibt der Bericht bei gleichem Inhalt (`ReportKey`: Kennung, Nummer der
  Vorschau, Beleg, neue Abschnitte) stehen (`PanelUi::begin_keeping`), nur Köpfe werden umgestylt: ↑/↓ 3–82 ms.
- **Punkt 3 (`app.rs`):** `Render` → `render_diag`, `Field(on)` → `field_focus`, `UiEv` gehen an die offene Diagnose
  (`diag_ev`), sonst an die Textaktionen; Tick setzt nur die Sekunden (kein Neuaufbau).
- **Punkt 4 (Eingabefeld):** `gtk::Entry` fest zwischen Kopf und Scrollbereich (`field_box`, nur in der Vorschau
  sichtbar, Länge `max_symptom_chars`). Fokus: der Entry selbst hat ihn nie (inneres `GtkText`), geprüft wird
  `StateFlags::FOCUS_WITHIN` (im Rauchtest belegt). Liegt der Fokus im Feld, gehen alle Tasten außer Esc, Return,
  KP_Enter, Tab, ISO_Left_Tab an das Feld und **nicht** an den Automaten (ein „c“ löste sonst Copy aus; Test im
  Rauchtest). `changed` meldet nur echte Eingabe mit Fokus, bei `set_text` zusätzlich blockiert; Klick hinein →
  `UiEv::FieldFocus` (`EventControllerFocus`). Beim Verbergen geleert. **Lesart zur Bestätigung (Daniel):** Auswahl im
  Feld setzt PRIMARY (GTK), Strg+V bzw. Mittelklick fügen aus CLIPBOARD bzw. PRIMARY ein, nur auf Handlung des Users im
  Feld (wie Tippen); dictate-ui liest dabei selbst nichts (Modulkopf `diag_view.rs`). Undo im Feld aus
  (`set_enable_undo(false)`: Strg+Z holte nach Schließen oder Sperre sonst ein altes Symptom zurück).
- **Punkt 5 (Öffnen vorgezogen):** `UiCmd::Diagnose` (`dictate-ui diagnose`): offene Diagnose → zu, offenes oder sich
  öffnendes Textaktionen-Panel → nur zu, gesperrt → verweigert mit Hinweis, sonst öffnen. Umgekehrt schließt Mod+I eine
  offene Diagnose, `ActMsg::Opened` öffnet nicht bei offener Diagnose. `locked()` schließt auch die Diagnose. Kürzel in
  der KDL bleiben Block C.
- **Mock und Rauchtest:** `tests/wayland/mock_daemon.py` spricht `diag_*` protokolltreu (Profile, Plan mit Index ab 0,
  Units, Fortschritt mit Index ab 1, Vorschau mit Schwärzung, Platzhaltern, Abwahl und Symptom, Senden mit `restore`,
  Copy, Abbruch durch Auflegen; Dateien `slow_diag`, `big_diag`; Log `diag_symptom <n>`, `diag_deselected <n>`).
  `tests/wayland/diag-shots.sh` (manuell, 52 Prüfungen, Bilder unter `/home/data/stt-test/logs/diag-shots/`): Ablauf
  Profile → Profil Unit (Suche, Liste, zurück) → Einträge → Fortschritt → Vorschau → Feld (Tab, Tippen mit „c“, Enter;
  Klick, Tippen, Esc) → Abwählen → Senden → Antwort → Werte → Beleg → Prüfrunde → Esc (`diag_clear`) → ein Panel zur
  Zeit (`diagnose` schließt die Textaktionen nur, `actions` die Diagnose nur, je mit `act_clear`/`diag_clear`, danach
  öffnet jedes wieder) → Fehleransicht (Datei `fail_collect`) → 48 KiB; am Ende kein Testtext (Bericht, Symptom,
  Klartext, Platzhalter) im Log von dictate-ui. Grundlage für `tests/wayland/diagnose.sh` in Block C.
- **Fund in der gemeinsamen Hülle (seit W3):** `window.dictate-panel label { color: inherit }` war spezifischer als die
  Klassenregeln, Farben wie `.dp-error`, `.dp-soft`, `.dp-meta`, `.dp-title` wirkten auf Labels nie (Fehler erschienen
  in Textfarbe, auch bei den Textaktionen). Jede Klassenregel steht jetzt unter `window.dictate-panel`; `wayland`
  danach unverändert grün (186).
- **Breite (offene Frage an Daniel, Versuch im verschachtelten niri):** `PanelUi::show` setzt vor jedem Zeigen
  `set_default_size(source.width())`; niri übernimmt das für das schwebende Fenster auch gegen `default-column-width`
  der Regel, je Quelle am selben Fenster (800 → 720 → 800 belegt). 800 für die Diagnose bräuchte also nur
  `Source::width()`, keine KDL-Änderung. Bis zur Antwort 720.
- **Prüfung:** `fmt,clippy`, `--lib ui::` 112, `--bin dictate-ui` 251, `wayland` 220 s (Last 3–4) und nach der
  CSS-Korrektur 123 s (186 ok), Rauchtest 52/52.
  Nicht gelaufen: `unit` (kein Daemon-Code geändert), `gui`, volle Suite. Dauer rund 35 min (Lesen, Bau, drei
  Rauchtest-Läufe); Token nicht gemessen.
- **Offen (gemeldet, nicht gebaut):** im Panel der Textaktionen (`fill_doc`, W3) haben Codeblöcke denselben
  Pango-Trennstrich beim Umbruch langer Befehle (ein Strich, der nicht im Befehl steht); Korrektur eine Zeile
  (`insert_hyphens="false"`), mit Daniel klären oder in Block C mitnehmen.
- **Für Block C (gebaut, aber nicht im Test):** Sperre schließt die Diagnose und Mod+Shift+I bei Sperre verweigert
  (`lock.sh` mit dem falschen logind erweitern); Copy report mit `c` (Inhalt von CLIPBOARD prüfen, ohne Klartext der
  Pseudonyme) und Stop durch Auflegen (Mock loggt `diag_collect hung up`) liefen im verschachtelten niri noch nie; der
  Hinweis „Lock detection is off“ erscheint nur bei den Textaktionen, nicht bei der Diagnose (mit Bericht und Klartext
  im Speicher): nachziehen. Abweichung vom Planwortlaut „in Links immer der Platzhalter“: die **Beschriftung** eines
  Links ist Fließtext (Klartext in der Ansicht „Real values“), nur das Linkziel bleibt Platzhalter; Daniel fragen.

**Entscheidungen Daniel (21.11., Beginn Block C), je mit Empfehlung vorgelegt:** (a) Diagnose **800** breit (nur
`Source::width()`, keine KDL-Änderung; Textaktionen bleiben 720). (b) Eingabefeld: Lesart bestätigt (Auswahl setzt
PRIMARY, Strg+V bzw. Mittelklick fügen ein, nur auf Handlung des Users; dictate-ui liest nichts). (c) Link-Beschriftung
in „Real values“ zeigt Klartext, nur das Linkziel bleibt Platzhalter (Abweichung vom Planwortlaut bestätigt).
(d) Pango-Trennstriche auch in Codeblöcken der Textaktionen abgestellt (mitgenommen).

**Stand Block C (Sitzung vom 21.11.2026, Branch `wayland-w4`, Commits `99b4af8`, `2e97875`, `32de1a0`, `94571d3`):**

- **Kürzel** (`99b4af8`): `Mod+Shift+I` → `spawn "dictate-ui" "diagnose"` in `niri/dictate-binds.kdl`; `niri validate` der
  Datei und der ganzen echten Konfiguration (Kopie im Scratchpad, mit der neuen Datei) gültig, `Mod+Shift+I` war in
  `~/.config/niri/` frei; Schritt `gui` grün (208 s). **Installation über chezmoi offen** (Zustimmung Daniel), und nur
  zusammen mit dem neuen `dictate-ui`: das installierte Binary kennt `diagnose` noch nicht.
- **Panel** (`2e97875`): Breite je Quelle (`WIDTH_DIAGNOSE` 800), `NO_HYPHENS` gemeinsam in `panel.rs` und auch in den
  Codeblöcken von `fill_doc`; Hinweis „Lock detection is off: the report and the answer are not cleared when the screen
  locks.“ in jeder Ansicht der Diagnose (`render_diag`), nachgezogen beim Wechsel der Erkennung (`Msg::Lock`).
- **Boot-Hinweis** (`32de1a0`): reiner Ablauf `ui::boot_hint` (`Timing` 10/20/20 s, `ATTEMPTS` 4, `Answer::{Show, Busy,
  DaemonDown, No}`, `Asker`; 5 Tests). Frage `diag::boot_hint` auf eigener Verbindung in einem Hilfsthread (nicht durch
  die Reihe des Panels; Frist 40 s; Socket fehlt oder Verbindung ohne Antwort zu → `DaemonDown`; Test gegen den Fake).
  In `app.rs`: `Boot { asker, pending, until, shown }`; ein „ja“ wartet, bis das Overlay frei ist (kein Hinweis, keine
  Aufnahme, kein Panel, nicht gesperrt: `overlay_free`), weil der Daemon den Boot schon gemerkt hat; die 20 s zählen ab
  dem ersten Zeigen; `handle` zeichnet nur neu, wenn sich Sichtbarkeit oder Warten ändern (nicht je Pegel). Overlay:
  `set_clickable` (nur solange der Hinweis gezeigt wird: `set_can_target`, Eingaberegion ganze Fläche, Zeiger „pointer“;
  `map`/`layout` lesen denselben Schalter), `on_click` (`GestureClick`, nur wenn anklickbar) → `Msg::BootClick` →
  `diagnose_key` (gleiche Verweigerung bei Sperre). Öffnen der Diagnose (Klick oder Kürzel) beendet den Hinweis.
  Testzeiten: versteckte Option `dictate-ui run --boot-hint-ms FIRST,RETRY,SHOW`. Lesart (Claude): ein Klick bei offenem
  Textaktionen-Panel verhält sich wie Mod+Shift+I (schließt dieses nur); praktisch nicht erreichbar, weil der Hinweis
  bei offenem Panel wartet.
- **Abnahme** (`94571d3`): `tests/wayland/diagnose.sh` in `run.sh` nach `actions.sh`, vor `lock.sh` (D1–D12):
  Mod+Shift+I öffnet schwebend, fokussiert, 800 breit, zweites schließt mit `diag_clear`; Ablauf bis zur Antwort, pstore
  als Abschnitt; Feld mit „c“ (kein Copy), Enter übernimmt; Copy report mit `c` nach „Real values“ und Sprung zum Beleg:
  CLIPBOARD byte-gleich dem Bericht des Mocks, ohne Klartext, Panel bleibt; Stop beim Sammeln (zurück, `diag_collect hung
  up`, `diag_cancel`) und beim Senden (`diag_send hung up`); Schließen mitten im Sammeln; ein Panel zur Zeit über beide
  Kürzel (Textaktionen 720); Fehleransicht; Boot-Hinweis: erste Frage mit echten Zeiten 9,5–13 s nach dem Neustart
  (gemessen 11,8 s samt Start), „schon geprüft“ fragt einmal; busy → fehlender Daemon → „ja“: drei Fragen, Overlay nimmt
  Klicks, Klick öffnet die Diagnose und kommt nicht beim Fenster darunter an, Hinweis kommt nicht wieder, Region wieder
  leer; höchstens vier Fragen bei busy; „ja“ während einer Aufnahme wartet (Overlay bleibt klick-durch), erscheint danach,
  läuft ab, danach klick-durch; Mod+Shift+I beendet den Hinweis. `lock.sh`: Sperre schließt die offene Diagnose und
  löscht, mitten im Sammeln aufgelegt (28b); Mod+Shift+I bei Sperre verweigert mit Hinweis im Overlay, nach dem
  Entsperren wieder (31b); ohne Erkennung der Hinweis auch in der Diagnose (33). Schlussprüfung von `run.sh` um
  `diagmarker`, `geheimhost`, `musterfrau`, Symptom, pstore-Abbild, `REDACTED`, `host-1`, `user-1` erweitert (über alle
  `ui*.log`, jeder Neustart hebt das Log auf). Mock: `diag_boot_hint` (Datei `boot_hint`, je Frage eine Zeile
  `show|busy|drop|no`), Auflegen auch geloggt, wenn erst die Antwort nicht mehr ankommt, pstore mit Abbild wie auf blub,
  `diag-copy.txt` für den Vergleich.
- **pstore** (Punkt 8): nichts gebaut; im Daemon belegt durch `pstore_*` in `src/diagnose/probes/tests.rs` (Abbild je
  Boot, Teile, unlesbar, Grenze bei Riesenabbild) und `kernel_command_line_is_strict_in_every_section`
  (`src/diagnose/filter.rs`: strenger Filter der Kommandozeile auch in pstore); im Panel als Abschnitt im Test.
- **Nachzug nach der Prüfung vor der Übergabe:** Klick auf den Boot-Hinweis zählt nur, solange er gezeigt wird
  (`boot.shown` statt `until`: ein später Klick bei inzwischen offenem Panel schloss sonst dieses); erste Frage gemessen ab
  dem Verbinden (`subscribe` im Mock), Fenster 9,5–11,5 s (gemessen 10,0 s); Abwahl eines Abschnitts und Prüfrunde
  (`check:1`, `diag_check`, Runde 2) im Schritt.
- **Befund (Block C, nicht behoben, Entscheidung Daniel 21.11.: nächste Sitzung zuerst):** Das Panel wächst nach dem
  Öffnen über niri (`set_window_height`) nach unten, die Oberkante bleibt, wo niri das schwebende Fenster mittig
  platziert hat; bei großer Antwort ragt es unten aus dem Bildschirm (Test mit 900 logischen Pixeln: Knopf „Run“ bei
  y=1004). Gilt für beide Quellen (seit W3). Korrektur: nach jeder Höhenänderung nur bei Überstand das eigene Fenster
  (`own_window`) per `Action::MoveFloatingWindow` so weit hochschieben, dass es passt (Lage aus `Windows`, Höhe des
  Ausgangs aus `Outputs`), kein Springen beim Streaming; Prüfung der Lage im Schritt `wayland` (Unterkante ≤ Höhe des
  Ausgangs) und D3b in `diagnose.sh` wieder per Klick auf `run` statt Enter.
- **Prüfung:** `fmt,clippy`, `--bin dictate-ui` 258, `--lib ui::` 118, `gui` 208 s, `wayland` 269 ok in 203 s (Last 9–12),
  nach dem Nachzug `fmt,clippy` und `wayland` 274 ok in 252 s (Last 12–17).
  Ein erster `wayland`-Lauf brach in Abschnitt 2 ab, bevor ein Teil von Block C geprüft wurde (das neue Binary lief
  schon; seine erste Boot-Frage kam rund 0,9 s vor dem Ende von niri, die erste gescheiterte Prüfung steht davor): das
  verschachtelte niri endete mit „Broken pipe“ zum kopflosen sway mitten in einer Tippausgabe, Last 12; Wiederholung
  ohne Änderung grün. Tritt es wieder auf, ist es ein Befund (`sway.log`, `niri.log` sichern). Nicht gelaufen: `unit` (kein Daemon-Code geändert), volle Suite. Dauer Block C rund 1,7 h
  (davon rund 25 min Prüfläufe); Token nicht gemessen.

**Auftrag nächste Sitzung (Abschluss W4, frisch, Branch `wayland-w4`):** (0) zuerst der Befund „Panel ragt aus dem
Bildschirm“ (oben, Korrektur und Prüfung wie beschrieben; Schritte `fmt,clippy,unit,wayland` bzw. gezielt
`--bin dictate-ui`). (1) volle Suite `scripts/check.sh` einmal
(ca. 15 min, allein). (2) Unabhängige Durchsicht Sicherheit (Subagent, Opus, frischer Blick auf den Code, nicht auf
diese Zusammenfassung): Bericht, Pseudonyme und Klartext (`restore`, Copy, Notizen), Logs, Symptom, Panel und Feld,
Sperre, Auflegen, Boot-Hinweis samt Eingaberegion des Overlays; Diff `main..wayland-w4`. Befunde beheben oder einzeln
Daniel vorlegen. (3) Doku: CLAUDE.md (Stand, Offen 5 und 8, Kürzel `Mod+Shift+I`, Tabelle der Prüfschritte nennt
`diagnose.sh`), `PLAN-diagnose.md` (Stand unter niri), README (Bedienung Diagnose, Boot-Hinweis). (4) Danach keine
neue Sitzung: Abschlussbericht an Daniel und Fragen nach Installation (alle Binaries, beide Dienste neu, KDL über chezmoi
gemeinsam), Merge nach `main` und Push.

**Stand Abschluss W4 (Sitzung vom 21.11.2026, Branch `wayland-w4`):**

- **(0) Panel ragt aus dem Bildschirm, behoben:** Spike im verschachtelten niri (Animationen an): `MoveFloatingWindow`
  mit `AdjustFixed` wirkt sofort, `tile_pos_in_workspace_view` meldet sofort die Ziellage, niri klemmt schwebende
  Fenster nicht auf den Ausgang; `SetFixed` zählt im Arbeitsbereich (über IPC unbekannt), darum relativ. Umsetzung in
  `niri.rs`: `placement` (Windows mit `app_id` **und** PID wie `own_window`, nur schwebend; Ausgang über `Workspaces`,
  logische Höhe aus `Outputs`), reine Rechnung `shift_into_view` (nur bei Überstand, genau um ihn hoch, Abstand
  `EDGE` 16, nie nach unten, also kein Springen beim Streaming), `move_window_by`; der Resizer in `panel.rs` ruft das
  nach jeder Höhe auf. Lesart (Claude): eine Leiste **unten** (exklusive Zone) kennt die Rechnung nicht, weil niri den
  Arbeitsbereich nicht meldet; die Leiste von Noctalia auf blub steht oben. Tests: 3 neue in `niri::tests` (Fake mit
  mehreren Antworten), `panel_inside` in `actions.sh` (Textaktionen nach dem Streaming) und in `diagnose.sh` D3b
  (Verschieben geloggt, Lage, Klick auf `run` statt Enter, Runde 2 bleibt drauf). Prüfung: `--bin dictate-ui niri::`
  6, `fmt,clippy`, `wayland` 279 ok in 328 s (Last 4–15); im Log z. B. `panel: move up 87 (top 222, height 748,
  output 899)`.
- **(1) Volle Suite** (`scripts/check.sh` auf dem Code von `2ae65b5`, allein, Last 11–13; `190be6c` kam während des
  Laufs als reiner Doku-Commit dazu): fmt, clippy, unit 613 s, wayland 279 ok
  in 308 s (kein „Broken pipe“), gui 119 s, **speech rot**: t18 (Absatz, 25 s) in 2 von 3 Läufen über der Schwelle
  (CER 0,104 und 0,094, max 0,09; Whisper ließ „übersetzen oder umformulieren“ aus, Lauf 2 fehlerfrei), Gesamt-CER
  0,0175 (max 0,04); Log `logs/check/20261121-193110`. W4 ändert am Sprachpfad nichts (Diff außerhalb von `src/ui`,
  `src/bin/dictate-ui`, `src/diagnose`: nur Symptom in `protocol.rs`, `sock_client.rs`, `daemon.rs`), Aufnahme,
  Testsatz und Schwellen seit Oktober unverändert. Ein Lauf `--only speech,gpu` danach grün (speech 241 s, CER 0,0054,
  t18 zweimal 0,0; gpu 105 s; `20261121-195341`). Befund: t18 wackelt; Schwellen nicht angefasst (Regel in
  `thresholds.tsv`), zur Entscheidung an Daniel.
- **(2) Durchsicht Sicherheit** (Subagent Opus, frischer Blick auf Code und Diff `main..wayland-w4`, nur lesend): nichts
  Hohes oder Mittleres; Punkte 1–3 und 5–8 der Prüfliste ok (Copy = gesendeter Text, Klartext nur im Fließtext, Logs
  ohne Inhalte, `Debug` ohne Inhalte, Symptom als Daten und gefiltert, Sperre und Auflegen, Eingaberegion nur bei
  gezeigtem Hinweis, `--boot-hint-ms` versteckt, begrenzt 1..=600000 ms, nur Zeiten: kein Risiko, niri-Aktionen nur
  auf das eigene Fenster). Befunde und Behebung:
  - F1 (low) Tab ins gefüllte Feld markierte den Inhalt (`grab_focus`, `gtk-entry-select-on-focus`) und bot das
    ungefilterte Symptom als PRIMARY an: jetzt `grab_focus_without_selecting`. Prüfung in `diagnose.sh` über
    `nested-clip watch-primary` (neu, zählt nur Meldungen); Gegenprobe mit dem alten Aufruf rot (PRIMARY änderte sich
    zweimal), mit der Korrektur grün.
  - F2 (low) Das Panel hatte keine eigene 30-min-Grenze (nur Leerlauf 300 s, jede Eingabe setzt ihn zurück): jetzt
    `expires_at` aus `expires_in_s` der Vorschau (Rückfall `SESSION_TTL` 30 min), `next_deadline` nimmt das Frühere,
    `tick` schließt (wipe, `diag_cancel`, `diag_clear`); neues Sammeln setzt die Frist zurück. Test
    `the_panel_closes_when_the_daemon_session_expires_even_while_in_use`. Dazu (Hinweis der Prüfung vor dem Abschluss): meldet
    irgendein Aufruf die Sitzung als unbekannt (`CallErr::unknown_session`, etwa nach Neustart des Daemons bei der
    Installation), löscht `reply` vorab Bericht, Antwort, ID und Frist (`forget_session`, ein wartendes Symptom wird
    überschrieben) und zeigt „The diagnosis expired“; vorher zeigten Copy, Abwahl und Symptom nur einen Hinweis und
    behielten die Daten. Test in `errors_have_guidance_and_a_way_back`.
  - F3 (info) Feld ohne `InputHints::PRIVATE`: gesetzt (Eingabemethoden lernen das Symptom nicht).
  - F4 (info, offene Lesart) Auswahl im Feld setzt PRIMARY, Strg+V/Mittelklick lesen CLIPBOARD/PRIMARY, nur auf
    Handlung des Users im Feld (Kopf von `diag_view.rs`): **von Daniel am 21.11. bestätigt** („so lassen“).
  - F5 (info) Kopien des Symptoms in `Request::to_line`/`cmd_line` werden nicht überschrieben, GTK-Kopien ohnehin
    nicht; kein Leck nach außen, nichts geändert.
  Prüfung danach: `fmt,clippy`, `--bin dictate-ui` 262, `--lib ui::` 119, `wayland` 281 ok in 268 s (Last 11–13).
  Außerdem: Seite „Diagnose“ des Einstellungsfensters sagte noch „Under niri the panel follows in a later version“
  (drei Stellen): jetzt Mod+Shift+I, Klick auf den Hinweis, Tab ins Feld. `actions.sh`: Lage mit `nested_wait 3`
  (das Verschieben läuft im Resizer-Thread).
- **Volle Suite auf dem endgültigen Stand** (nach den Korrekturen der Durchsicht, `c2bc8bb`, Last 12 → 25): fmt,
  clippy, unit 704 s grün; **wayland rot** (275 ok, 7 Fehler; `logs/check/20261121-202746`): (a) Klicks nach dem
  Wachsen des Panels trafen daneben (✕ bei y=254 statt 155): `item_xy` las die Lage, bevor der Resizer-Thread das
  Panel hochgeschoben hatte, also ein Wettlauf, den das Verschieben aus (0) in den Test gebracht hat; Folgefehler
  „back to the answer“, „diag_check“. (b) `restart_ui`: GTK brauchte bei Last 25 allein 6,0 s bis „ready“, der Test
  wartete 5 s auf die Verbindung. Das „Broken pipe“ in `niri.log` war diesmal niri → IPC-Client (Antwort an einen
  aufgelegten Helfer), nicht niri → sway wie in Block C. Korrektur `0b2dca3`: `panel_still` (zwei gleiche Lesungen im
  Abstand von 0,3 s) vor jedem Klickziel, Neustart wartet bis 15 s. Danach `--only wayland,gui,speech,gpu`: wayland 281
  ok in 292 s (Last 18–20), **gui rot** (5 Prüfungen der Ping-Ziele: die neue Tastenzeile Tab und der längere Text
  zum Boot-Hinweis auf der Seite „Diagnose“ verschoben „Add target“, weil der Test vom Seitenende zurückscrollt, von
  y 494 auf 437); Korrektur `023985c`, gui 112 s grün; speech 242 s (t18 zweimal 0,0, damit dreimal in Folge
  fehlerfrei) und gpu 103 s grün (`20261121-205721`). Nachzug: `panel_still` allein reichte nicht sicher (zwischen
  neuer Höhe und Hochschieben stand die Lage rund 0,5 s still, ragte aber über), darum wartet `item_xy` auf
  `panel_settled` (`panel_inside` und `panel_still`); wayland 281 ok in 251 s (Last 10–18, `20261121-210619`). Damit
  ist jeder Schritt auf dem Code des Endstands grün, aber nicht in einem einzigen Durchlauf (nach `c2bc8bb` änderten
  sich nur Testskripte).
- **Lesart (Claude), für den Live-Test:** Beim Streaming rückt das Panel schrittweise hoch (im Test 87, dann 12 px),
  unter Last etwa 0,5 s nach der neuen Höhe; die Bedienflächen wandern dabei unter dem Zeiger. Kein Test prüft
  „ruhig“. Alternative, falls es stört: erst nach dem Ende des Streamings einmal auf die Endlage schieben. Nach dem
  Hochschieben rückt das Panel beim Schrumpfen nicht wieder herunter.
- **Dauer und Token:** Sitzung rund 2 h (davon rund 75 min Prüfläufe, Last 10–25); Durchsicht (Opus) rund 209k Token
  und 10 min; Token der Hauptsitzung nicht gemessen.
- **(3) Doku** (`190be6c`): README (Kürzel, Bedienung unter niri, Feld, Boot-Hinweis, Sperre, pstore-Freigabe, Kürzel nur
  mit passendem `dictate-ui`), CLAUDE.md (Stand, Kürzel, Offen 5 und 8, Prüfschritt `wayland` nennt `diagnose.sh`,
  niri-Regel Verschieben), Archiv-Vermerk „Stand unter niri“ in `PLAN-diagnose.md`.
- **Entscheidungen Daniel (21.11., Abschluss):** installieren (alle Binaries vom Stand des Merges, beide Dienste neu,
  danach KDL über chezmoi), Fast-Forward nach `main`, `main` pushen, Lesart F4 so lassen. t18: Schwelle bleibt,
  beobachten (Empfehlung, kein Widerspruch).
- **Umgesetzt (21.11., 21:13):** Fast-Forward `main` auf `37bdc36`, `cargo install` (alle drei Binaries), beide
  Dienste neu gestartet (aktiv, `dictate-ui` verbunden; die Sitzung war beim Start gesperrt, „locked at start“),
  `niri/dictate-binds.kdl` vorher mit der ganzen echten niri-Konfiguration als Kopie geprüft (gültig) und über chezmoi
  übernommen (Branch `dictate-diagnose-binds` im Dotfile-Repo, Merge nach `master`, nicht gepusht; `config.kdl` dort
  `MM`, nicht angefasst), `main` nach `origin` gepusht. Offen: Live-Test durch Daniel.
- **(4) Offen für Daniel (vor den Entscheidungen):** Installation (alle Binaries, beide Dienste neu, `niri/dictate-binds.kdl` über chezmoi nur
  zusammen mit dem neuen `dictate-ui`), Merge nach `main`, Push; Lesart F4; Umgang mit t18. Vorige Sitzung
  (`0f92632a`) war `done`; ihr Chat-Tab bleibt (die Orca-CLI kann Chats nicht schließen).

## 6f. Auftrag W7, vorgezogen: AwesomeWM entfernen (eigener Block nach W3, frische Sitzung)

Entscheidung 15 (Daniel, 20.11.): „Alles was AwesomeWM ist, kann weg“, vorgezogen vor die Live-Abnahme, als eigener
Block nach dem Abschluss von W3. Grundlage: Aufstellung `docs/w7-awesome-inventar.md` (Tabellen A–F). Gelöscht wird
erst nach Daniels Antworten auf die Fragen unten.

- **Branch:** `remove-awesome` von `main` (nach dem Merge von W1–W3), eigener Orca-Worktree.
- **Sicher drin (aus Tabelle A):** `awesome/` ganz (vorher `fake-logind` nach `tests/wayland/` umziehen),
  `--format awesome` (`Format::Awesome`, `to_awesome`, `esc`, Awesome-Arme in `client.rs`) samt Tests, Schritt `xephyr`
  in `check.sh`, sichtbare Awesome-Texte im Einstellungsfenster (Kürzel-Tabellen auf niri umstellen) und in den
  KI-Prompts (`diagnose/ai.rs`, `palette/ai.rs`: „Arch Linux, Wayland, niri“), README neu für `dictate-ui`/niri,
  CLAUDE.md, DESIGN.md, ROADMAP.md; Fensterregel für `dictate-settings` in `niri/dictate-binds.kdl`.
- **Entschieden (Daniel, 20.11. abends, je einzeln gefragt):**
  1. **Umfang: Awesome und X11.** Auch `keys.rs`, `paste.rs`, `Xclip`, `x11rb`, Schritt `xclip` (dictate läuft danach
     nur unter Wayland/niri; `TIMEOUT`/`MAX_BYTES` aus `selection.rs` bleiben für `primary.rs`).
  2. **CLIs: alles entfernen,** was nur Lua aufrief oder ohne Aufrufer bleibt (`act`, `paste`, `synth`,
     `release-keys`, `listen`, `start`/`stop`/`toggle`/`cancel`/`status`/`shutdown`, `dictate-palette`,
     `dictate-diag`). Bleiben: `dictate daemon` (Dienst), `dictate-ui`, `dictate-settings`. Vorher prüfen, was
     Dienste, Tests und `dictate-ui cancel` ohne Oberfläche (`link::cancel_direct`, Socket direkt) brauchen.
  3. **Probe `xorg_log` entfernen** (Katalog, Profil `last_crash`, Tests, Fehlalarm-Korpus prüfen).
  4. **`overlay.transparency` vereinfachen** auf an/aus; alte Werte beim Laden übersetzen, Seite im
     Einstellungsfenster anpassen.
  5. **Tests des Einstellungsfensters ins verschachtelte niri** (Harness `tests/wayland`, Klicks über den Injektor);
     danach eframe ohne Feature `x11`, kein Xvfb mehr in `check.sh`.
  6. **Historische Pläne: Archiv-Vermerk** oben je ein Satz, Inhalt unverändert.
  7. **chezmoi: Awesome ganz entfernen** (Awesome-Config samt dictate-Symlink und `rc.lua`, Theme, Menü, picom,
     `~/.xinitrc`); nur über chezmoi, vorher den Stand zeigen und das echte Display nie anfassen.
  8. **W5-Vorlagen bis W5 behalten:** `json.lua`, `palette_url.lua`, `palette_url_test.lua` nach `c552237:docs/w5-vorlagen/`.
- **Umfang damit größer als geschätzt:** zwei Sitzungen (a: Awesome, X11, CLIs, Probe, Transparenz, Doku; b: GUI-Tests
  ins verschachtelte niri, chezmoi), je Teil committen.
- **Folge:** Palette und Diagnose-Panel sind bis W4/W5 nicht bedienbar (Daemon-Teile bleiben).
- **Prüfung:** volle Suite (ohne `xephyr`, ggf. ohne `xclip`), Bericht 5–12 Zeilen.

### Stand W7a (Branch `remove-awesome`, Sitzung vom 20.11.2026)

Reihenfolge abweichend vom Auftrag: `Xclip` rufen nur `dictate act grab/copy` und `dictate diag copy` auf, darum fällt
es mit Teil 4 (CLIs), nicht mit Teil 3; `--format awesome` (Teil 2) wird nach Teil 4 entfernt, wenn `client.rs` schon
weg ist (sonst Umbau einer Datei, die danach ganz fällt).

- **Teil 1 fertig** (`3b956da`): `awesome/` gelöscht, `fake-logind` nach `tests/wayland/` (Pfade in `run.sh`,
  `lock.sh`), Vorlagen nach `c552237:docs/w5-vorlagen/` (mit README, `palette_url_test.lua` läuft dort: 56 Prüfungen),
  Schritt `xephyr` aus `scripts/check.sh`, Kommentare auf die Ports in `src/ui/` umgestellt.
- **Teil 3 fertig** (`21db263`): `src/keys.rs`, `src/paste.rs`, `dictate paste|synth|release-keys`, `x11rb` als direkte
  Abhängigkeit (bleibt bis Block b transitiv über eframe/winit im Lockfile), `tests/paste_x.rs`.
- **Teil 4 fertig** (`8723fd2`): `dictate` hat nur noch `daemon` und `transcribe` (Offline-Test des Modells; nicht im Auftrag genannt,
  kein Awesome/X11, darum geblieben). Gelöscht: `src/client.rs`, `src/palette/client.rs`, `src/diagnose/client.rs`,
  `dictate-palette`, `dictate-diag`, `Xclip` (`src/actions/selection.rs` behält nur `TIMEOUT`, `MAX_BYTES`), Schritt
  `xclip`, die Tests `act_cli.rs`, `palette_cli.rs`, `diag_cli.rs`, `daemon_client.rs`. Vorher je Test geprüft (lesender
  Agent), ob er Daemon-Verhalten prüft, das sonst nirgends getestet ist: 33 von 37 nur CLI, die Daemon-Teile der übrigen
  stehen in `daemon_actions.rs`, `daemon_palette.rs`, `daemon_diagnose.rs`; einzige Lücke, die Zeilengrenze von
  `pal_resolve`/`pal_ask` an der Kante (512 KiB), ist als `pal_resolve_and_pal_ask_lines_up_to_the_limit_are_read`
  nachgezogen. `listen --format awesome` in `daemon_protocol.rs` ist jetzt ein JSON-Abonnententest (Overlay- und
  Terminal-Vorgaben im Strom), `send_at` dort über den Rohsocket. `DIAG_COLLECT_REPLY_TIMEOUT` steht jetzt in
  `protocol.rs` (W4 braucht es, der Test im Daemon bleibt). `sock_client.rs` ist nur noch für `dictate-ui` (ohne
  `emit`, `fail_json`, `refused_json`, nicht mehr in der Bibliothek). `dictate-ui cancel` ohne Oberfläche
  (`link::cancel_direct`) spricht den Socket selbst und brauchte nichts davon; die Dienste rufen nur `dictate daemon` und
  `dictate-ui run`.
  - Befund am Rand (nicht geändert): `Context::from_json` der Palette gibt die serde-Meldung durch; ein Wert falschen
    Typs (etwa `{"clients":[{"id":"…"}]}`) könnte darin wörtlich zurückkommen. Für W5 prüfen.
  - `run_limit` in `dictate-ui/act.rs` (Frist aus `actions.timeout_ms` plus Reserve, dann `act_cancel`) hat keinen
    eigenen Rust-Test mehr; bisher prüfte das Muster nur `act_cli.rs` am CLI.
- **Teil 2 fertig** (`8de381b`, nach Teil 4): `esc`, `Event::to_awesome` samt Tests; Cargo-Beschreibung nennt niri.
- **Teil 5 fertig** (`ada9a25`): Probe `xorg_log` aus Katalog (33 Proben), Profil `last_crash` (8 Einträge;
  `kernel_info` rückt auf Position 7, die `skip`-Listen in `daemon_diagnose.rs` angepasst), Fixtures und Tests.
  Fehlalarm-Korpus unverändert grün (die Xorg-Zeilen darin sind Journal-Inhalt). Fall 2 der Live-Messung
  (`silent-hang`) prüft jetzt die letzte Zeile des temp-loggers (09:46:40) statt der Zeit aus dem Xorg-Log
  (Archiv-Vermerk in `diagnose-ai.md`). Der Mechanismus `~` in `Exec::Read` bleibt (nutzt heute kein Eintrag).
- **Teil 6 fertig** (`764b601`): `overlay.transparency` ist ein Wahrheitswert (Vorgabe `true`); `none` lädt als
  `false`, `auto`/`argb`/`window` als `true`, andere Texte sind ein Fehler; `render_toml` entfernt den alten Text,
  danach steht nur eine Abweichung als `transparency = false` in der Datei (Test
  `old_transparency_modes_load_as_on_or_off_and_save_as_a_flag`). Einstellungsfenster: Schalter See-through/Solid.
- **Teil 7:** Einstellungsfenster (Kürzel-Tabellen auf niri, `app_id` statt `WM_CLASS`, Hilfe `niri msg windows`,
  Palette und Diagnose „no panel under niri yet“), KI-Prompts (`palette/ai.rs`: „niri on Wayland, Linux“,
  `diagnose/ai.rs`: „Arch Linux, Wayland, niri“; `INPUT_GUARD`/`ASK_FRAME` unverändert, die Awesome-Absichten der
  Palette bleiben bis 3.7/W5), Fensterregel `dictate-settings` (schwebend) in `niri/dictate-binds.kdl` (`niri validate`
  grün), Rust-Doku (`WM_CLASS` → `app_id`, Hinweise auf Awesome in der Gegenwart), README neu für `dictate-ui`/niri,
  CLAUDE.md, DESIGN.md (nur noch Einstellungsfenster bis W6), ROADMAP.md, Archiv-Vermerke in `SPEC-v2.md`, `PLAN.md`,
  `PLAN-actions.md`, `PLAN-palette.md`, `PLAN-diagnose.md`, `UX-actions.md`, `diagnose-ai.md`.
- **Durchsicht** (unabhängiger Agent, nur lesend): nichts Gemeinsames gelöscht (`#[path]`-Einbindungen von
  `dictate-ui`/`dictate-settings`, Units, KDL, `check.sh`, Lockfile geprüft), Sicherheitsregeln in CLAUDE.md nicht
  abgeschwächt, Migration von `transparency` korrekt, keine weiteren festen Positionen oder Probenzahlen. Nachgearbeitet
  (`de0fd6e`): widersprüchliche GUI-Texte zu `Mod+Space`/`Mod+Shift+i`, Klickziele in `tests/gui/diagnose.sh` (die
  Seite Diagnose ist unten kürzer, „Add target“ von y 431 auf 488), klare Meldung bei falschem Typ von `transparency`,
  Verweise auf Lua/Awesome in der Gegenwart, Archiv-Vermerk in `palette-ai.md` (Prompt geändert), und als Ersatz für
  die Lua-Prüfung des Xephyr-Tests der Quelltext-Test `no_shell_anywhere` in `dictate-ui` (kein `sh -c`, kein Programm
  aus einer Variable, keine glib/gio-Kommandozeilen, kein `spawn-sh` in der KDL).
- **Prüfung W7a:** volle Suite grün am 21.11. (`scripts/check.sh`: fmt, clippy, unit 620 s, wayland 161 s, gui 74 s,
  speech 241 s mit CER 0,0067, gpu 105 s; zusammen 1204 s, Last der Maschine 15–23 durch eine andere Sitzung). Nicht
  installiert, nicht gepusht, nichts nach `main`; chezmoi und `~/.config` unberührt.

### Auftrag W7b (nächste frische Sitzung, Branch von `remove-awesome` bzw. nach dessen Merge von `main`)

1. **GUI-Tests ins verschachtelte niri** (Entscheidung 5): `tests/gui/smoke.sh`, `diagnose.sh`, `interact.sh`,
   `shots.sh` laufen heute auf Xvfb `:98` mit `xdotool` und `import`. Umbau auf den Harness `tests/wayland/nested`
   (kopfloses sway, eigener Laufzeitordner, Klicks über den Injektor aus W1–W3, Screenshots über den Harness), gegen
   `tests/gui/mock_daemon.py`. Danach eframe ohne Feature `x11` (`Cargo.toml`), Schritt `gui` ohne Xvfb, `display_free`
   und die Xvfb-Teile von `check.sh` und `tests/gui/run-gui.sh`/`stop-gui.sh` entfernen; prüfen, ob `x11rb` dann ganz
   aus `Cargo.lock` fällt. Prüfung: `fmt,clippy,gui,wayland`, dann volle Suite.
2. **chezmoi: Awesome ganz entfernen** (Entscheidung 7), nur mit Daniel: vorher den Stand zeigen (`chezmoi managed`,
   `chezmoi diff` für Awesome-Config samt dictate-Symlink und `rc.lua`-Block, Theme-Segment, Menüeintrag,
   `~/.config/picom/picom.conf` mit `_DICTATE_OVERLAY@`, `~/.xinitrc` mit `awesome --no-argb`), dann nur über chezmoi
   ändern, das echte Display nie anfassen. Dazu die alten Binaries `~/.cargo/bin/dictate-palette` und `dictate-diag`
   (nach Daniels Zustimmung löschen; `cargo install` entfernt sie nicht).
3. **Installation (nur mit Daniels Zustimmung):** alle drei Binaries zusammen installieren und beide Dienste neu
   starten (das Event `overlay` trägt `transparency` jetzt als Wahrheitswert; ein altes `dictate-ui` liest es nicht),
   `niri/dictate-binds.kdl` über chezmoi übernehmen (Fensterregel `dictate-settings`).
4. Offen aus W7a zur Entscheidung: `packaging/dictate-settings.desktop` `StartupWMClass` (unter Wayland ohne Wirkung,
   schadet nicht), Befund `Context::from_json` (serde-Meldung mit Wert) für W5.

### Stand W7b (Branch `w7b-gui-niri` von `remove-awesome`, Sitzung vom 21.11.2026)

- **Teil 1 fertig** (`978e9cd`, Doku und `x11` in `8c2809c`): die GUI-Tests laufen im verschachtelten niri.
  `tests/gui/lib.sh` (neu) bringt den Harness hoch (Fensterregeln aus `niri/dictate-binds.kdl` per include, also auch
  „dictate-settings schwebend“), startet Mock und Fenster, findet das Fenster über `app_id` **und** PID, prüft 1180x860
  schwebend, bevor geklickt wird, und schneidet Screenshots (grim) auf das Fenster. `run-gui.sh`/`stop-gui.sh` sind
  entfallen: der Harness hält den Injektor über Dateideskriptoren des Skripts, Hoch- und Abbau gehören darum in ein
  Skript (Abweichung vom Auftrag, der nur ihre Xvfb-Teile entfernen wollte). Abbau per trap, ein Abbau mit übrigen
  Prozessen ist ein Fehler; `DICTATE_SOCKET` entfernt, `XDG_RUNTIME_DIR` unter `wl-run` geprüft.
  - **Ausgabe 2600x1950 bei 1,625** (1600x1200 logisch, gebrochen wie auf blub): bei 900 logisch öffnete niri das
    Fenster nur 731 hoch, die Klickziele brauchen 860. Belegung `us`, weil der Injektor evdev-Codes an US-Positionen
    drückt (`de` tauscht y/z und verschiebt `-`).
  - **Injektor:** neuer Schritt `scroll:N` (Mausrad über den virtuellen Zeiger: Quelle, diskreter Schritt, je Raste ein
    Frame), `.` und `-` in `text:`. Gemessen: eine Raste sind 40 logische Pixel, linear; unter Xvfb war sie größer
    (40 Rasten erreichten dort das Seitenende, hier nicht; die Weite dort nicht gemessen). Darum `diagnose.sh` neu kalibriert: Ende (80 Rasten), dann
    16 zurück, „Add target“ bei y 494 statt 488; Cloud-Screenshot nach 12 statt 11 Rasten. `interact.sh` traf das Feld
    Timeout schon vor W7b nicht mehr (y 725 statt 554), nachgezogen.
  - `check.sh`: Schritt `gui` ohne Xvfb/xdotool/import, `need_nested` (Programme des Harness, Bau des Injektors) für
    `wayland` und `gui`, `display_free`, `GUI_T` und der Aufruf von `stop-gui.sh` entfernt.
  - **eframe ohne Feature `x11`:** fallen weg `x11-dl`, `glutin_glx_sys`, `as-raw-xcb-connection`. **`x11rb` bleibt**
    im Lockfile: arboard (über egui-winit, Feature `clipboard`) hängt unter Linux bedingungslos davon ab; `clipboard`
    bleibt (Kopieren/Einfügen in Textfeldern des Fensters), unter Wayland nimmt egui-winit smithay-clipboard.
  - Prüfung: `fmt,clippy,gui,wayland` grün (gui 89 s, wayland 170 s), nach dem Wegfall von `x11` `fmt,clippy,gui` grün.
  - Abbruch geprüft: TERM an `check.sh --only gui` mitten in `diagnose.sh` hinterließ nichts (Mock, Fenster, sway,
    niri weg, `wl-run` leer). `shots.sh` mit `MOCK_CLOUD=1` lief (28 Bilder, Cloud-Hinweise sichtbar). Befund am Rand:
    die Seite Actions zeigt gegen den Mock gleich nach dem Öffnen „Unsaved changes“, so schon unter Xvfb in W7a.
  - Volle Suite grün am 21.11. (fmt, clippy, unit 521 s, wayland 155 s, gui 82 s, speech 241 s mit CER 0,0075, gpu
    103 s; zusammen 1105 s).
- **Teil 2 fertig** (Daniel, 21.11., je einzeln gefragt: „Awesome ganz“, „auch in ~ löschen“): in der chezmoi-Quelle
  Branch `remove-awesome`, gemergt (`b8d2149`). Per `chezmoi destroy` einzeln entfernt (Quelle und Home, 208
  Einträge): `~/.config/awesome` samt Symlinks `dictate`, `claude_usage`, `continuity` (Ziele unberührt), lain,
  freedesktop, doorcam, Theme qubit, `rc.lua`; `~/.config/picom/picom.conf`, `autostart/picom.desktop`, `~/.xinitrc`,
  `awesome-session.target` samt Wants. Nicht pauschal auf Ordner, weil darin **unverwaltete** Dateien liegen; die
  blieben (in `~/.config/awesome`: `.gitignore`, `archive/`, `cryptocurrency-icons/`, `icons/`, zwei `rc.lua.bak-*`,
  in `themes/qubit`: `neg.frag`, `theme.lua.bak-*`; in `~/.config/picom`: zwei Backups). Leere verwaltete Ordner
  entfernt, die übrigen per `chezmoi forget`. README der chezmoi-Quelle angepasst. Fremde offene chezmoi-Diffs
  (flameshot, `niri/config.kdl`, noctalia, `.zshenv`, doorcam-watch, ydotoold) unberührt, kein pauschales `apply`.
  `~/.cargo/bin/dictate-palette` und `dictate-diag` gelöscht.
- **Teil 3 fertig** (Daniel, 21.11.): `cargo install --path . --locked` aus `w7b-gui-niri` (alle drei Binaries),
  `daemon-reload`, `dictate.service` und `dictate-ui.service` neu gestartet (beide aktiv, keine Fehler im Journal).
  `niri/dictate-binds.kdl` (Fensterregel `dictate-settings`) vorher gegen eine Kopie der echten niri-Konfiguration
  validiert, dann in der chezmoi-Quelle (Branch `dictate-settings-rule`, gemergt) und gezielt
  `chezmoi apply ~/.config/niri/dictate-binds.kdl`; niri lud die Konfiguration ohne Fehler neu.
- **Nach W7b, Entscheidungen Daniel vom 21.11. (je einzeln gefragt):**
  - `remove-awesome` und `w7b-gui-niri` nach `main` gemergt (`b78a377`) und gepusht.
  - Units an `awesome-session.target` in chezmoi entfernt (Quelle und `~`, Branches `remove-x11-units`,
    `remove-doorcam-watch`, gemergt): `idle-display-off.service`, `x0vncserver.service`, Drop-in
    `clipmenud.service.d/display.conf`, dazu `doorcam-watch.service` (die Fassung in `~` war schon auf niri umgebaut, nur
    die Quelle veraltet; auf Nachfrage trotzdem entfernt, Aktivierung in `graphical-session.target.wants` aufgehoben).
    Unberührt: `~/.local/bin/doorcam-watch` und `doorcam-binds.kdl` der niri-Konfiguration.
  - Unverwaltete Reste archiviert und gelöscht: `~/.config/awesome` und `~/.config/picom` nach
    `/home/data/archiv/awesome-picom-reste-2026-11-21.tar.gz` (3255 Dateien), dazu die alten Konfigurationen
    `~/.config/awesome_backup` und `awesome_old` (Mai 2017) nach `/home/data/archiv/awesome-2017-backup-old-2026-11-21.tar.gz`
    (1040 Dateien); Dateizahl je Archiv vor dem Löschen geprüft.
  - `StartupWMClass` aus `packaging/dictate-settings.desktop` entfernt (Branch `w7-nacharbeiten`). Die Palette ordnet
    Fenster dann über die Desktop-ID zu (`palette/resolve.rs`), die der `app_id` entspricht.
  - „Unsaved changes“ beim Öffnen der Seite Actions: `Settings::from_value` ergänzt fehlende eingebaute Prompts schon
    beim Laden (`fill_builtin_prompts`, sortiert wie `pairs`), Test `missing_builtin_prompts_are_filled_on_load`;
    `tests/gui/smoke.sh` klickt auf jeder Seite Apply und verlangt, dass kein `set_config` ankommt (Gegenprobe: das
    alte Fenster fiel genau auf Actions durch).
  - **Für W5:** `Context::from_json` der Palette gibt bei falschem Typ die serde-Meldung samt Wert durch; in W5 nur Feld
    und erwarteten Typ melden, Test mit einem Wert falschen Typs.
  - Offen: Live-Test von W2/W3 und des Einstellungsfensters unter niri (schwebend nach der neuen Regel).
- **Offene Fragen, je einzeln mit Empfehlung (Daniel, 21.11. mittags):**
  - Leistensymbol (Tabelle 6b, `widget.lua`): Noctalia-Schaltfläche `~/.config/noctalia/dictate.toml` (custom_button,
    Glyph `microphone`, links `dictate-ui actions`, rechts `dictate-settings`, feste Befehle; Noctalia führt `exec` über
    die Shell aus), in beiden `start`-Leisten nach `workspaces`; über chezmoi (`config.toml` dabei mit dem laufenden
    Stand aus `~` übernommen), `noctalia config validate` grün, Sicherung `config.toml.pre-dictate-button-2026-11-21`.
  - Verhaltensänderungen aus W3 (Sperre verwirft Diktat und Ausgabe, Mod+I bei Sperre verweigert, Tippen stoppt beim
    Öffnen des Panels, Lage-/Hover-/Klickzeilen im Debug-Log als Testhaken): bestätigt.
  - Noctalia nach Updates (aus W0): pacman-Hook `/etc/pacman.d/hooks/91-noctalia-restart-hint.hook` gibt nur einen
    Hinweis aus, startet nichts neu.
  - Ollama lokal: nicht einrichten. `to_de` „Dear Sam“ → „Lieber Sam“: so lassen.
  - Textaktionen: Fehler der KI nur als Code und feste Meldung (`AiError::Stream` neu für Fehler mitten im Strom,
    `act_run` über `client_text`). Einstellungsfenster: `palette.model` folgt dem Anbieterwechsel.
  - Veraltet und gestrichen: „nicht gestreamter KI-Pfad lässt einen Thread laufen“ (seit 23.10. erledigt,
    `PLAN-actions.md`), „Super beim Halten“ (am 20.11. entschieden).

## 6g. Auftrag W5: Befehlspalette unter niri (Entscheidung 6, 3.7)

Stand vorher (21.11.): W1–W4 und W7 gemergt, gepusht und installiert (`main` `101c7b1`); Branch `wayland-w5` von
`roadmap-w4` (`ccb2d3d`, ROADMAP nach W4; kommt mit W5 nach `main`). Live-Test von W2–W4 durch Daniel offen; Befunde
daraus haben Vorrang. Grundlage dieses Auftrags: Aufstellung aus dem Daemon-Teil (`src/palette/`), den Lua-Quellen vor W7
(`git show 30cad69:awesome/dictate/palette.lua` bzw. `palette_exec.lua`, `src/palette/client.rs`) und `c552237:docs/w5-vorlagen/`
(Sitzung vom 21.11.; Zeilennummern unten: `exec:` = `palette_exec.lua`, `pal:` = `palette.lua`, Stand `30cad69`).
Kennzeichnung wie oben: **gemessen**, **abgeleitet**, **offen**.

### Ziel

Die Palette ist unter niri wieder bedienbar: eine Taste öffnet ein Panel in `dictate-ui` (Fenster `dictate-panel`,
Quelle `palette`), Eingabe mit lokalem Resolver bei jedem Tastendruck, KI auf Return/Ctrl+Return, Stimme ins Panel,
Ausführung jeder Absicht des Katalogs mit einer niri-Umsetzung. Der Daemon-Teil (`pal_resolve`, `pal_ask`,
`pal_chosen`, `pal_sources_refresh`, `pal_frecency_clear`, `target:"palette"`) bleibt; er wird nur dort geändert, wo
Kontext und Katalog Awesome-Begriffe tragen. Der frühere CLI-Weg (stdin) entfällt: `dictate-ui` schreibt die Zeile
selbst auf den Socket (`sock_client::Conn`, Muster `act.rs`/`diag.rs`).

### Absichten und ihre Umsetzung unter niri (26, `src/palette/catalog.rs` 136–333, `intent.rs` 144–223)

„Vorlage En“ = Entscheidung Daniels unten; ohne Vermerk ist die Umsetzung eindeutig oder Lesart von Claude.

| Absicht | Klasse | unter Awesome (`exec:`) | unter niri |
|---|---|---|---|
| `view_tag` | run | `t:view_only()` (268) | `FocusWorkspace{reference: Id}` mit der ID aus dem Kandidaten (`Target::Workspace`, L10; die Zahl löst der Daemon auf dem Monitor des Zielfensters auf); Vorlage E2 (Arbeitsflächen statt Tags) |
| `focus_window` | run | `activate` mit Tag/Screen (278, 138) | `FocusWindow{id}` mit der ID aus dem Kandidaten; niri nimmt die Arbeitsfläche mit (**gemessen** in D2, P22; Monitor nicht belegt, ein Ausgang) |
| `move_window_to_tag` | run | `cl:move_to_tag` (287) | `MoveWindowToWorkspace{window_id: Some(id), reference: Id(workspace), focus: false}` mit beiden IDs aus `Target::MoveWindow` (L10); Vorlage E2 |
| `previous_tag` | run | `awful.tag.history.restore` (353) | `FocusWorkspacePrevious`; Vorlage E2 (Name) |
| `set_layout` | run | `awful.layout.set` (300) | **kein Gegenstück**, Vorlage E5 |
| `toggle_window` floating | run | `cl.floating` (331) | `ToggleWindowFloating{id}` (wie Mod+Shift+Space, gemessen in `coach-binds.kdl`) |
| `toggle_window` fullscreen | run | `cl.fullscreen` | `FullscreenWindow{id}` (wie Mod+F) |
| `toggle_window` maximized | run | `cl.maximized` | `MaximizeWindowToEdges{id}` (wie Mod+Shift+M) |
| `toggle_window` sticky | run | `cl.sticky` | **kein Gegenstück** (niri-ipc 26.4 kennt es nicht), Vorlage E3 |
| `toggle_window` ontop | run | `cl.ontop` | **kein Gegenstück**, Vorlage E4 |
| `close_window` | confirm | `cl:kill()` (344) | `CloseWindow{id: Some(id)}` |
| `launch_app` | run | argv aus `.desktop`, `env -C`, `Terminal=true` → `alacritty -e` (359) | argv aus `.desktop` (Daemon, `split_exec`), gestartet nach Vorlage E10, Terminal nach E11 |
| `open_project` | run | `alacritty --working-directory <p> -e claude` (397) | dasselbe argv, gestartet nach E10, Terminal nach E11 |
| `open_folder` | run | `xdg-open <p>` (408) | `xdg-open <p>` nach E10 (startet den Dateimanager, langlebig) |
| `open_url` | run | `valid_web_url`, `xdg-open` (415) | dasselbe, nach E10; URL in `dictate-ui` erneut geprüft (Doppelprüfung bleibt, `c552237:docs/w5-vorlagen/palette_url.lua` 38) |
| `web_search` | run | `search_url`, `xdg-open` (424) | dasselbe; Kodierung nach RFC 3986 in Rust neu (Vorlage `palette_url.lua` 18, Tests aus `palette_url_test.lua`) |
| `lock` | run | xss-lock → `loginctl lock-session` (435) | `noctalia msg session lock` (dieselbe Zeile wie Daniels Sperrtaste in der niri-Config, gemessen); Lesart L5 |
| `volume` | run | `pactl … @DEFAULT_SINK@`, Schritt bis 100 % (464, `sink_percent` 453) | unverändert `pactl` (läuft gegen PipeWire); Lesart L6 |
| `media` | run | `playerctl play-pause/next/previous` (499) | unverändert; Lesart L6 |
| `show_widget` doorcam | run | Awesome-Modul `doorcam.toggle()` (523) | Vorlage E6 |
| `show_widget` claude_usage | run | Awesome-Modul `claude_usage.toggle_popup()` | Vorlage E7 |
| `show_widget` clipboard_history | run | `clipmenu` mit rofi (X11) | Vorlage E8 |
| `show_widget` emoji | run | `rofi -modi emoji -show emoji` | Vorlage E9 |
| `dictate_settings` | run | `dictate-settings --page` | vorhandenes `open_settings(page)` (`app.rs` 985) |
| `screenshot` full/window/area | run | flameshot mit Monitorrechnung (609, 587) | `ScreenshotScreen`, `ScreenshotWindow{id}` (ID aus dem Kandidaten bzw. dem Fenster vor dem Öffnen), `Screenshot` (niris Auswahl); wie Print/Ctrl+Print/Alt+Print (gemessen). `full` und `area` mit Abgang (Entscheidung 17, Teil D3): Panel erst unsichtbar (Deckkraft 1/255, Titel „dictate: leaving“ mit Fensterregel in der KDL), dann aufnehmen, dann schließen; **gemessen** pixelgleich zum Bild ohne Panel, auch mit 3 s Schließ-Animation (0 von 8, Esc-Gegenprobe 8 von 8). `window` schließt zuerst wie bisher |
| `diagnose` | ai | `diagnose_panel{profile,…}` (700) | Diagnose-Panel mit Vorauswahl: `DiagPanel::open(Some(preset))` (`diag_panel.rs` 1002, `preset_of` 766 schon vorbereitet) |
| `logout` | confirm | `awesome.quit()` (634) | `Quit{skip_confirmation: true}` (die Palette hat schon bestätigt); Lesart L5 |
| `reboot`, `poweroff`, `suspend` | confirm | `systemctl …` (640–651) | unverändert `systemctl`; Lesart L5 (`suspend` ist auf blub per `sleep.conf` gesperrt, die Fehlermeldung wird gezeigt) |
| `ask` | ai | Textpanel mit Quelle `question` (674) | Panel der Textaktionen mit Quelle `question` im selben Fenster; `act.rs` 216 sendet heute fest `Source::Selection` |
| `text_action` | ai | Textpanel `selection` mit `preselect` (687) | dasselbe; `ui::panel` braucht `preselect`; lokal weiter nur über die KI erreichbar |
| `explain_clipboard` | ai | CLI las CLIPBOARD | neuer CLIPBOARD-Leser in `dictate-ui` nur für diese Absicht (Lesart L8) |

### Sicherheitsregeln (gelten für jeden Block)

1. **Fester Katalog, lokale Auflösung:** das Modell liefert nur eine Absicht mit Suchbegriffen; Ziele (Fenster,
   Arbeitsflächen, Programme, Projekte) löst der Daemon gegen den Kontext und lokale Listen auf. Kein Treffer = keine
   Ausführung, nie eine Ersatzwahl.
2. **Feste ID:** jede niri-Aktion nur mit `id: Some(…)` aus dem gewählten Kandidaten bzw. dem Fenster, das vor dem
   Öffnen fokussiert war (gemerkt wie `act::capture_focus`); nie „das fokussierte Fenster“ (das ist das Panel selbst).
   Das eigene Panel (`own_window`: `app_id` **und** PID) fehlt im Kontext. Vor der Aktion prüft der Ausführer, ob die
   ID noch in `Windows` steht; sonst „That window is gone“ (wie `exec:` 95–104).
3. **Nur argv, nie Shell:** Programme nur als Argumentliste (`valid_argv`: 1–128 Einträge, kein NUL, `argv[0]` nicht
   leer; `find_exe` gegen den Suchpfad der Sitzung, Ergebnis absolut). Kein `sh -c`, kein `spawn-sh`. Modelltext
   erreicht nie ein argv-Element außer als geprüfte URL bzw. URL-kodierter Suchbegriff (`open_url`, `web_search`).
   Die Tests `no_shell_anywhere` und `sources_stay_apart` (`primary.rs`) gelten für jedes neue Modul und werden für den
   Startweg (E10) angepasst, nicht gelockert: ein dynamisches `argv[0]` geht nur durch die eine Startfunktion.
4. **Nie loggen, nie in argv von `dictate-ui`:** Eingabe, Kontext (Fenstertitel), KI-Antworten und CLIPBOARD-Inhalt
   nie ins Log (auch nicht mit `DICTATE_DEBUG`), nie in `history.rs`; im Log nur Absicht, Klasse, Zahlen. Die
   Log-Prüfung am Ende von `tests/wayland/run.sh` bekommt die Testtexte der Palette.
5. **Fehlermeldungen ohne Werte:** `Context::from_json` (`resolve/context.rs` 153) meldet bei falschem Typ heute die
   serde-Meldung samt Wert an den Aufrufer (abgeleitet: serde zitiert Werte; nicht ins Log, aber in die Antwortzeile).
   Neu: nur Feldpfad und erwarteter Typ, Test mit einem Wert falschen Typs, der in der Meldung nicht vorkommt.
6. **Bestätigung:** `confirm` braucht ein zweites Return, auch bei lokalem Treffer; nach Stimme wird nie automatisch
   ausgeführt; KI-Vorschläge stehen als Vorschau und laufen erst mit Return.
7. **Sperre:** schließt das Panel, bricht KI und Stimme ab, verweigert das Öffnen (Hinweis im Overlay), wie W3/W4.

### Panel in `dictate-ui`

- **Automat rein in `src/ui/`** (ohne GTK, Eingaben mit `now`, Folgen als `Effect`), Name ohne Verwechslung mit der
  Farbpalette `ui::palette`, z. B. `ui::cmdpal`. Zustände und Regeln aus `pal:` (Zeilen in 6b und im Bericht der
  Sitzung): Eingabe, Kandidaten (höchstens 12, `valid_candidate` 293), `shown_for`, Auswahl, `confirm`, eine Anfrage
  unterwegs plus eine wartende mit `seq`/`gen` (227–290), `enter_asks` (350), `start_ask`/`cancel_ask` mit Ticker und
  Abbruch durch Auflegen (421, 372), Vorschau der KI bleibt bis zur nächsten Änderung (304), `choose`/`want_choose`
  (583), `enter_confirm` (568), Verlauf 50 Einträge nur im Speicher mit `history_step` (499, 1295), `one_space` (1066),
  Tasten nach `on_key` (1364; Mod+E-Weiterleitung entfällt, niri-Kürzel greifen vor dem Fenster, im verschachtelten
  niri prüfen), Totzeit und `stale`-Tasten über `ui::gate::Gate`. „The command palette is off“ neutral, nicht rot.
- **Fenster:** Quelle `palette` in der gemeinsamen Hülle `PanelUi` (`panel.rs`: Kopf mit ✕, Zeilen, Fuß, Hover über
  `Effect::Highlight`, Lage-Log für Klicktests, Höhe über niri). Eingabezeile als `gtk::Entry` wie das Feld „What is
  wrong?“ (`grab_focus_without_selecting`, `InputHints::PRIVATE`, keine Markierung als PRIMARY), Fuß mit Tastenhinweisen
  (`pal:` 1164–1287). Ein Panel zur Zeit: die Taste der Palette schließt ein offenes Textaktionen- oder Diagnose-Panel
  und umgekehrt; Fokusverlust schließt wie bei den anderen Quellen.
- **Daemon-Anbindung:** `pal_*` über einen eigenen Arbeiter (Muster `diag.rs`), Kontext aus niri-IPC (`Windows`,
  `Workspaces`) beim Öffnen, Fristen nach Lesart L3, Größe nach Lesart L4.
- **Stimme im Panel** (6b Befund 3 und 5): Mod+E bei offenem Panel startet mit `target:"palette"` (`Commander` kennt
  das noch nicht), das Overlay bleibt aus (nur Fehler), Partial/Final gehen in die Eingabezeile (`one_space`, 1000
  Zeichen), nie ausführen; Esc bricht die Aufnahme ab, ein spätes `final` nach dem Schließen wird verworfen.
- **Weiter an die anderen Panels:** `ask`, `text_action`, `explain_clipboard` und `diagnose` schließen die Palette und
  öffnen im selben Fenster die passende Quelle; `pal_chosen` erst, wenn das Ziel-Panel offen ist (wie `on_success`).
- **Kürzel:** neue Taste nach E1 in `niri/dictate-binds.kdl` (`spawn "dictate-ui" "palette"`), Unterbefehl `palette`
  in `ui::ctl`; die installierte Datei über chezmoi nur mit Daniels Zustimmung.

### Vorlagen an Daniel (je einzeln, mit Empfehlung)

Stand der Belegung: in `~/.config/niri` (`config.kdl` mit `coach-binds.kdl`, `dictate-binds.kdl`, `doorcam-binds.kdl`,
`noctalia*.kdl`, Belegung `de`) sind 104 Kombinationen belegt; frei sind u. a. **Mod+Alt+Space, Mod+B, Mod+N, Mod+T,
Mod+U, Mod+Y, Mod+Z** (gemessen per grep, ohne `niri msg`).

- **E1 Taste.** Empfehlung **Mod+Alt+Space**: am nächsten an Mod+Space aus Awesome (Gewohnheit), frei. Nachteil: die
  Nachbarn Mod+Shift+Space und Mod+Ctrl+Space schalten Schweben bzw. den Fokus zwischen Schweben und Kacheln (harmlos,
  umkehrbar). Alternative: Mod+U oder Mod+T (frei, ohne Nachbarn mit Wirkung).
- **E2 Arbeitsflächen statt Tags.** niri hat Arbeitsflächen je Monitor, dynamisch (am Ende immer eine leere), `idx`
  ab 1, optional benannt. Empfehlung: (a) die Absichten heißen `view_workspace`, `move_window_to_workspace`,
  `previous_workspace`; (b) eine Zahl meint `idx` auf dem Monitor des Zielfensters, Treffer nur für vorhandene
  Arbeitsflächen plus die leere am Ende („7“ bei drei belegten = kein Treffer, Meldung), Name nur für benannte;
  (c) Präfixe „tag“, „workspace“, „arbeitsfläche“ bleiben Synonyme; (d) Kontext: `workspaces[]` (`id`, `idx`, `name`,
  `output`, `active`) statt `tags[]`, Fenster mit `app_id` statt `class`/`instance`, Arbeitsfläche als `workspace_id`,
  `layouts[]` entfällt; (e) Frecency: alte Einträge `view_tag:<n>` usw. werden beim Laden auf die neuen Namen
  abgebildet (die Datei enthält nur Ziel-IDs und Zähler). Folgen: Katalogtexte, Schema und Modell-Prompt für alle
  Anbieter (Größe der strict-Grammatik bleibt etwa gleich, `examples_are_valid_for_schema_and_validation` mitziehen),
  `resolve/tests.rs` (1388 Zeilen) und `tests/daemon_palette.rs` umstellen. Alternative: interne Namen behalten und nur
  Texte ändern (weniger Umbau, aber „tag“ bliebe im Protokoll und im Prompt falsch).
- **E3 `sticky`.** Empfehlung **streichen** (Wert aus `toggle_window.what`, Schlüsselwort „sticky“/„on all tags“
  liefert keinen Treffer mehr): niri kennt keine Fenster auf allen Arbeitsflächen.
- **E4 `ontop`.** Empfehlung **streichen**: schwebende Fenster liegen unter niri immer über den gekachelten; ein
  eigener Schalter existiert nicht, und auf „floating“ abzubilden änderte die Anordnung unerwartet.
- **E5 `set_layout`.** Empfehlung **ersetzen** durch `column_display` mit `normal`/`tabbed` (`SetColumnDisplay`, wie
  Mod+Space): die nächste niri-Entsprechung eines Layouts, passt zu Entscheidung 6 „voll portieren“. Alternative:
  streichen (Mod+Space und die Palette von niri-coach decken es ab).
- **E6 `doorcam`.** Empfehlung **ersetzen** durch `doorcam-watch --toggle` (vorhanden, dieselbe Zeile wie Mod+Shift+D in
  `doorcam-binds.kdl`, gemessen).
- **E7 `claude_usage`.** Empfehlung **streichen**: `awesome-claude-usage` ist reines Awesome-Lua (gemessen: kein
  Wayland-Teil im Repo). Ein Ersatz wäre ein eigenes Noctalia-Plugin, eigenes Projekt, nicht W5.
- **E8 `clipboard_history`.** Empfehlung **ersetzen** durch `noctalia msg panel-toggle clipboard` (dieselbe Zeile wie
  Mod+Shift+V, gemessen); clipmenu ist X11.
- **E9 `emoji`.** Empfehlung **behalten mit `rofi -modi emoji -show emoji`**: rofi 2.1.1 und rofi-emoji 4.1.0 sind
  installiert, rofi läuft unter niri (Mod+Shift+X, gemessen); das Einfügen von rofi-emoji unter Wayland läuft über
  `wtype` (installiert; abgeleitet, live prüfen). Alternative: Emoji-Anbieter des Noctalia-Launchers (vorhanden laut
  Binary, ein direkter Aufruf ist **offen**).
- **E10 Startweg für Programme.** Empfehlung **niri-IPC `Action::Spawn`** für alles, was ein langlebiges Programm
  startet (`launch_app`, `open_project`, `open_folder`, `open_url`, `web_search`, rofi, `doorcam-watch`): niri legt
  gestartete Programme in eigene Scopes `app-niri-*.scope` (gemessen an laufenden Programmen, für IPC-Spawn
  abgeleitet), sie überleben damit einen Neustart von `dictate-ui.service` (Vorgabe `KillMode=control-group` beendete
  sonst alle Kinder). Kein dynamisches `Command::new` in `dictate-ui`. Folgen: niri meldet keinen Startfehler (darum
  `find_exe` und absoluter Pfad vorher Pflicht; „Erfolg“ für `pal_chosen` heißt „übergeben“), kein Arbeitsordner
  (`Path=` aus dem Eintrag über `/usr/bin/env -C <dir> -- argv…`, `argv[0]` ohne `=`; `open_project` nutzt
  `--working-directory` des Terminals). Kurzläufer mit Rückmeldung (`pactl`, `playerctl`, `systemctl`, `noctalia msg`)
  bleiben Kindprozesse mit festem Programmnamen. Ein Spike im verschachtelten niri klärt in Block A, ob der Prozess
  außerhalb des Cgroups von `dictate-ui` landet (höchstens zwei Ansätze). Alternative: eigener Start über
  `systemd-run --user --scope` (Startfehler sichtbar, aber zweiter Weg mit Ausnahme im Test).
- **E11 Terminal.** Empfehlung **fest `alacritty`** (wie Mod+Return), `--working-directory <dir> -e argv…`:
  `xdg-terminal-exec` fehlt auf blub, und Terminals unterscheiden sich in diesen Argumenten; ein Config-Schlüssel
  bräuchte eine argv-Vorlage mit Platzhaltern (mehr Angriffsfläche, Seite im Einstellungsfenster). Alternative:
  Schlüssel `palette.terminal` mit Vorgabe `alacritty`.
- **E12 `Request::parse`** (`protocol.rs` 354) zitiert serde ebenso (`"bad request: {e}"`, Test 927 mit `"input":5`),
  außerhalb des Vermerks zu `from_json`. Empfehlung: in Block A gleich mitbeheben (gleiche Klasse, gleicher Test), sonst
  als offener Punkt.

### Lesarten von Claude (bei Widerspruch korrigieren)

- **L1** Die Palette bekommt den Leerlauf von 300 s aus `Gate::IDLE` wie die anderen Panels (unter Awesome gab es
  keinen; im Daemon gibt es für `pal_*` keine Sitzung).
- **L2** Die Lage des Folge-Panels (`ctx.place`) und das Ziehen entfallen: dasselbe Fenster, niri legt es.
- **L3** Fristen aus einer Quelle im Rust-Client: `pal_resolve` 10 s (der erste Aufruf liest Programme und Projekte
  von der Platte), `pal_ask` `palette.timeout_ms` plus 10 s, Abbruch durch Auflegen.
- **L4** Kontextgröße: das Panel kappt nach den Grenzen aus `context.rs` (Titel 512, `app_id` 128, höchstens 500
  Fenster, nach Zeichen) **und** hält das Zeilenbudget des Daemons (512 KiB) ein, notfalls mit weniger Fenstern
  (zuletzt fokussierte zuerst, `focus_timestamp`).
- **L5** System: Sperre über `noctalia msg session lock` (wie Daniels Taste; ob Noctalia `loginctl lock-session`
  beachtet, ist ungeprüft), Abmelden über niri `Quit`, Neustart/Ausschalten/Schlafen über `systemctl` wie bisher.
- **L6** Lautstärke und Medien bleiben bei `pactl`/`playerctl` (unabhängig von der Shell, mit Rückmeldung und der
  Grenze 100 %); Noctalias `volume-*` zeigte zwar ein OSD, gibt aber keinen Stand zurück.
- **L7** `pal_chosen` nur nach Erfolg: bei niri-Aktionen nach `Handled`, bei Spawn nach der Übergabe, bei Panels erst,
  wenn das Ziel-Panel offen ist.
- **L8** `explain_clipboard`: CLIPBOARD wird nur auf diese Absicht über Data-Control gelesen, mit Frist und Grenze wie
  PRIMARY; trägt sie `x-kde-passwordManagerHint: secret`, wird nur der Hinweis gelesen und abgelehnt („The clipboard
  holds a password“), wie Entscheidung 10. Inhalt nie loggen, nur über die Vorschau an den Daemon.

Aus Block A (21.11., bei Widerspruch korrigieren):

- **L9** Der Kontext trägt je Arbeitsfläche zusätzlich `focused` (`niri_ipc::Workspace::is_focused`), über E2 (d)
  hinaus: ohne fokussiertes Fenster (leere Arbeitsfläche) ist der Monitor einer Zahl sonst bei mehreren Monitoren nicht
  bestimmbar. Reihenfolge: Monitor des Zielfensters, sonst der fokussierten Arbeitsfläche, sonst der einzige Monitor;
  sonst trifft keine Zahl (ein Name weiter). Die Liste heißt `windows` statt `clients` (die Felder ändern sich ohnehin).
- **L10** Ziel ist immer die feste niri-ID: `Target::Workspace { id }` für `view_workspace`, `Target::MoveWindow { id,
  workspace }` für `move_window_to_workspace` (beide IDs, die Zahl ist je Fenster auf dessen Monitor aufgelöst; zwei
  gefundene Fenster auf zwei Monitoren wandern auf verschiedene Arbeitsflächen). Der Ausführer (Block B) nimmt
  `WorkspaceReferenceArg::Id`, nie `Index` (der zählte auf dem fokussierten Monitor) und löst nie neu auf.
- **L11** Ohne Liste der Arbeitsflächen gibt es keine ID und damit keinen Treffer (unter Awesome galt jede Zahl 1 bis
  9). Ein Fenster mit unbekannter `workspace_id` gilt als ohne Arbeitsfläche (zwei IPC-Abfragen sind nicht atomar).
  Zahlen 1 bis 255 (`catalog::MAX_WORKSPACE_INDEX` = `u8::MAX` wie `idx` in niri), eine Quelle für Schema, Prüfung,
  Resolver und Kontext. Ohne Präfix tragen nur die Arbeitsflächen des fokussierten Monitors ihre Nummer („3“,
  „workspace 3“), benannte auf jedem Monitor ihren Namen; eine unbenannte auf einem anderen Monitor ist nur über ihre
  Fenster erreichbar.
- **L12** Häufigkeit: Ziel-ID einer Arbeitsfläche ist ihr Name, sonst die Nummer (so passen abgebildete alte Einträge
  `view_tag:<n>` auf unbenannte). Beim Laden abgebildet: `view_tag`, `move_window_to_tag`, `previous_tag` (auch die
  Ziel-ID `previous_tag` des Schlüsselworts); verworfen: `set_layout:*`, `toggle_window:sticky|ontop`,
  `show_widget:claude_usage` und jede Absicht, die der Katalog nicht kennt. Format bleibt Version 1.
- **L13** Meldungen ohne Werte (Regel 5, E12) ohne neue Abhängigkeit: `Request::parse` und `Context::from_json` lesen
  erst `serde_json::Value`, prüfen die Form selbst gegen eine Feldtabelle (Pfad nur aus bekannten Feldnamen und
  Indizes, nie ein fremder Schlüssel) und geben serde erst danach; scheitert serde trotzdem, kommt eine feste Meldung.
  Ein Test liest `RawRequest` aus dem Quelltext und hält die Tabelle `REQUEST_FIELDS` gleich.
  `serde_path_to_error` hätte nur den Pfad geliefert, die Meldung von serde mit dem Wert aber nicht ersetzt.

Aus Block B (22.11., bei Widerspruch korrigieren):

- **L14** `logout`: `pal_chosen` geht **vor** `Quit` (niri beendet die Sitzung und damit `dictate-ui`; danach käme die
  Wahl nie an). Scheitert `Quit`, ist die Wahl trotzdem gezählt, wie unter Awesome (`exec:` 634). Weicht von L7 ab.
- **L15** Aktionen ohne ID: `SetColumnDisplay` wirkt nur auf die fokussierte Spalte. Der Ausführer fokussiert erst das
  Zielfenster (`FocusWindow{id}`), prüft mit `FocusedWindow`, dass genau dieses den Fokus hat, und setzt erst dann die
  Darstellung; sonst nichts („The window did not take the focus“). `FocusWorkspacePrevious`, `ScreenshotScreen` und
  `Screenshot` wirken naturgemäß auf den fokussierten Monitor. niri meldet den Fokus sofort nach `Handled`: **gemessen** in
  D2 (P23), 36 von 36 Aufrufen in sechs Läufen von `wayland`/Wrapper, Ziel jeweils auf einer anderen Arbeitsfläche.
- **L16** Kontext: Budget des JSON 256 KiB (`ui::pal_context::CONTEXT_BUDGET`; die ganze Zeile von `pal_resolve` mit
  längster Eingabe und größter Umgebung bleibt unter 512 KiB, Test `the_worst_line_fits_the_daemon`). Arbeitsflächen
  bekommen höchstens ein Viertel davon (Namen voller Steuerzeichen hätten sonst alle Fenster verdrängt, gefunden im
  Spiegeltest), dann fallen die am längsten nicht fokussierten Fenster. Der Fokus ist das vor dem Öffnen gemerkte
  Fenster, nie das aus niri (dort wäre es das Panel).
- **L17** `dictate_settings` startet über die Naht des Ausführers und niri `Spawn` (`dictate-settings [--page p]`),
  nicht über das vorhandene `open_settings` (Kindprozess von `dictate-ui`, Tabelle oben): gleicher Grund wie E10
  (überlebt einen Neustart von `dictate-ui`) und Fakes im Test. `open_settings` für Mod+Ctrl+Shift+E bleibt unverändert.
- **L18** Abwehr in der Tiefe im Ausführer: Absichten mit Pflicht zur Bestätigung (laut Katalog, Spiegeltest) laufen nur
  mit `ExecCtx::confirmed`, auch wenn der Kandidat eine andere Klasse trägt; `screenshot` `window` kommt vom Daemon ohne
  Ziel und nimmt das vor dem Öffnen fokussierte Fenster; alle anderen Fensterabsichten nur mit `Target::Window` (der
  Daemon liefert es immer, `on_focused_window` in `resolve.rs`), ohne Rückgriff.

Aus Block C (22.11., bei Widerspruch korrigieren; Regeln aus `palette.lua` in `c552237:docs/w5-vorlagen/cmdpal-spec.md`):

- **L19** Totzeit beim Öffnen: `Gate::open` wie bei den anderen Panels (300 ms), unter Awesome gab es beim Öffnen keine.
  Sie sperrt nur Return, Ctrl+Return und Klicks, nie Tippen, Pfeile, Tab oder Esc (wie `fresh()` in Lua).
- **L20** Blättern im Verlauf (`history_step`) vergisst auch eine wartende Wahl (`want_choose`); in Lua führte eine
  späte Antwort sonst die Eingabe aus dem Verlauf aus bzw. fragte für sie die KI.
- **L21** Der Kontext entsteht einmal beim Öffnen (Block B, vor dem Zeigen des Panels), nicht je Anfrage wie in Lua;
  der Ausführer prüft vor jeder Fensteraktion, ob das Fenster noch da ist.
- **L22** Tests erreichen nie echte Programme: das versteckte Flag `dictate-ui run --fake-programs DIR` ersetzt den
  Suchpfad des Ausführers ganz (ein vorangestellter Ordner im `PATH` fände ein fehlendes Fake weiter hinten echt);
  `run.sh` startet `dictate-ui` nur damit.
- **L23** Stimme im Panel geht ohne `app` an den Daemon (fokussiert ist das Panel selbst, ein Profil je Programm passte
  nicht); der Besitz der Aufnahme wird im Hauptthread beim Senden entschieden (die Ereignisse tragen kein `target`).
- **L24** Ctrl+C und Ctrl+G bleiben der Eingabezeile (GTK: Kopieren der eigenen Markierung), sie schließen nicht wie
  der Prompt unter Awesome; eine Markierung in der Zeile setzt PRIMARY nur auf Handlung des Users (Lesart wie das Feld
  „What is wrong?“ in W4).
- **L25** Meldungen unter niri statt der Texte des CLI: „No answer from dictate“ / „Type to try again.“, „dictate sent an
  invalid answer“; die übrigen Texte wie in Lua bzw. unverändert aus `error` des Daemons.
- **L26** (mit D1 entfallen: die Übergaben sind gebaut, L34) `Outcome::Handoff` (ask, text_action, explain_clipboard, diagnose) zeigt bis Block D im Overlay „Command
  palette: this command follows in a later version“ und meldet kein `pal_chosen`.

### Zuschnitt (je Block eine frische Sitzung)

| Block | Inhalt | Prüfung | gesperrt durch |
|---|---|---|---|
| **A** Daemon und Spike | Kontext für niri (E2), Katalog und Schlüsselwörter (E2–E9), Frecency-Abbildung, Prompt und Beispiele, `from_json` ohne Wert mit Test (und E12), `resolve/tests.rs`, `tests/daemon_palette.rs`; Spike Startweg (E10) im verschachtelten niri, Ergebnis `docs/spikes/wayland-spawn.md` | `fmt,clippy,unit` | E2–E10, E12 |
| **B** Ausführer und Kontext | `niri.rs` erweitert (`Windows`, `Workspaces`, `FocusWindow`, `CloseWindow`, `FocusWorkspace`, `FocusWorkspacePrevious`, `MoveWindowToWorkspace`, Schalter, `SetColumnDisplay`, `Screenshot*`, `Spawn`, `Quit`), Kontext aus niri ohne eigenes Panel (L4), Ausführer-Modul (Ziel nur aus dem Kandidaten, Prüfung auf vorhandenes Fenster, `done` genau einmal, `valid_argv`/`find_exe`, URL- und Suchprüfung doppelt, Startweg nach E10/E11); **eine** Naht für Programme: der Ausführer löst jedes Programm über einen Suchpfad auf, den Tests auf einen Ordner mit Fake-Programmen legen (Muster `ProbeEnv::with_bin_dir`), keine fest verdrahteten absoluten Pfade außer `/usr/bin/env` | `fmt,clippy`, `cargo test --release --bin dictate-ui`, `--lib ui::…` | E10, E11 |
| **C** Automat, Fenster, Taste, Stimme | `ui::cmdpal` mit Unit-Tests aus `pal:`, Quelle `palette` in `PanelUi` mit Eingabezeile, `pal_*`-Arbeiter (L3), Sperre, Leerlauf (L1), Unterbefehl und Taste (E1), Stimme ins Panel; erste Fassung `tests/wayland/palette.sh` (Öffnen, Tippen, Kandidaten, Return, Esc, confirm, Sperre) | `fmt,clippy,unit,wayland` | E1 |
| **D** Übergänge und Abnahme | Quellen `question`, `clipboard`, `preselect` im Textaktionen-Panel, CLIPBOARD-Leser (L8), Diagnose mit Vorauswahl, `palette.sh` vollständig: jede Absicht gegen Mock-Daemon und Fake-Programme, niri-Aktionen im verschachtelten niri (Fokus über Arbeitsflächen, Screenshot ohne Panel, `Quit` nur dort), Log-Prüfung um die Testtexte | `fmt,clippy,unit,wayland` | – |
| **E** Abschluss | volle Suite, unabhängige Durchsicht Sicherheit (argv, Startweg, Kontext, CLIPBOARD, Logs, Sperre), Doku (CLAUDE.md, README mit Taste, ROADMAP, Vermerk in `PLAN-palette.md`), `docs/w5-vorlagen/` löschen; danach Fragen nach Installation, Merge und Push | volle Suite | – |

**Tests erreichen nie die echten Programme:** `palette.sh` legt den Suchpfad des Ausführers auf einen Ordner mit
Fake-Programmen (`systemctl`, `loginctl`, `noctalia`, `pactl`, `playerctl`, `xdg-open`, `doorcam-watch`, `rofi`,
`alacritty`, die nur ihr argv in eine Datei schreiben); `run.sh` bricht ohne diesen Ordner ab wie ohne privaten Bus.
`confirm`-Absichten laufen nur gegen die Fakes, `Quit` nur gegen das verschachtelte niri, nie `niri msg` an das echte.

**Schätzung:** fünf Sitzungen (A–E), B und D je eine bis zwei.

### Stand W5

- 21.11.: Auftrag geschrieben (dieser Abschnitt); Vorlagen E1–E12 an Daniel, Lesarten L1–L8 zur Kenntnis. Kein Code.
- 21.11.: **Daniel hat alle Empfehlungen freigegeben** („Freigegeben“, Entscheidung 16 in Abschnitt 7): E1 Mod+Alt+Space,
  E2 Arbeitsflächen wie empfohlen (a–e), E3/E4/E7 streichen, E5 ersetzen durch `column_display`, E6 `doorcam-watch
  --toggle`, E8 `noctalia msg panel-toggle clipboard`, E9 rofi-emoji, E10 niri `Spawn` (Spike in Block A), E11 fest
  `alacritty`, E12 in Block A mitbeheben; Lesarten L1–L8 gelten. Nächster Schritt: Block A in frischer Sitzung.
- 21.11.: **Block A fertig** (Commits `5206241`, `dc69b6a`, dazu Spike und Plan). `scripts/check.sh --only
  fmt,clippy,unit` grün (1486 Tests, 747 s).
  - E12: `Request::parse` prüft die Form vor serde (`REQUEST_FIELDS`, Test liest `RawRequest` aus dem Quelltext),
    Meldung nur Feld und Typ; Test mit Markern in Wert, Schlüssel und Syntaxfehler (Lesart L13).
  - E2: Kontext `workspaces[]` (`id`, `idx`, `name`, `output`, `active`, `focused`) und `windows[]` (`id`, `app_id`,
    `title`, `workspace_id`, `focused`), `layouts` entfällt; `Context::from_json` meldet Pfad und Typ ohne Wert
    (Regel 5). Absichten `view_workspace`, `move_window_to_workspace`, `previous_workspace`, `column_display`
    (`normal`/`tabbed`); Ziele `Target::Workspace { id }`, `Target::MoveWindow { id, workspace }` (L9–L11). Präfixe
    und Wendungen „tag“, „workspace“, „arbeitsfläche“; „last workspace“ bewusst nicht (Initialen „lw“ schlugen
    LibreOffice Writer).
  - E3/E4/E7: `sticky`, `ontop`, `claude_usage` gestrichen (auch Schlüsselwörter); E6/E8/E9 bleiben als Werte von
    `show_widget` (Ausführung Block B). Katalogtext, Prompt („workspaces“), flaches Schema (`workspace` statt `tag`),
    Beispiele und Tests mitgezogen.
  - Frecency: Abbildung und Verwerfen beim Laden (L12), Test `awesome_names_are_mapped_on_load`.
  - **Spike E10** (`docs/spikes/wayland-spawn.md`, ein Ansatz): ein über IPC `Spawn` gestarteter Prozess liegt im
    Cgroup von niri, nicht des Aufrufers, hat nicht den Aufrufer als Eltern und überlebt den Stopp einer Aufrufer-Unit
    mit `KillMode=control-group` (gemessen); Umgebung und Arbeitsordner kommen von niri, `env -C` wirkt; ein fehlendes
    Programm meldet niri nicht (`Handled`, kein Log). In der echten Sitzung liegen per Kürzel gestartete Programme in
    `app-niri-<name>-<pid>.scope` (gelesen); für IPC abgeleitet, Prüfung beim Live-Test offen. Folgen für B und D:
    `find_exe` mit absolutem Pfad Pflicht, Terminal mit festem Befehl (`-e`), `palette.sh` prüft am Ende auf
    Prozesse mit dem Laufzeitordner (über `Spawn` gestartete sind keine Kinder von niri; die Login-Shell startete auf
    blub zellij, das den Lauf überlebte).
  - Gemessen (Regel „Messen statt glauben“): Block A brauchte eine Sitzung wie geschätzt, rund 1 Stunde (Zeitstempel der
    Logs 22:43–23:40), davon etwa 20 min CUDA-Erstbau des Worktrees (parallel zum Schreiben) und 12 min `unit`; Token
    nicht gemessen (die Sitzung sieht ihren Verbrauch nicht).
  - Offen für Block E oder W6: die Seite „Palette“ des Einstellungsfensters sagt noch „windows and tags“
    (`src/gui/pages/palette.rs` 79) und „Under niri the panel follows in a later version“.
  - Nächster Schritt: Block B (Ausführer und Kontext in `dictate-ui`).
- 22.11.: **Block B fertig** (Commits `92dd9c8`, `4a3c6c6`, Nachtrag UTF-8). Geprüft auf dem letzten Stand:
  `scripts/check.sh --only fmt,clippy` grün, `cargo test --release --bin dictate-ui` (302) und `--lib -- ui::
  palette::` (355) grün; `unit` ganz nicht gelaufen (Block E). Lesarten L14–L18 oben. Suchpfad und Ordner nur mit
  UTF-8-Pfaden (argv an niri ist Text; ein verlustbehaftet umgewandelter Pfad hätte als „gestartet“ gezählt).
  - `src/ui/` (ohne GTK, ohne niri-Typen, auch in der Bibliothek): `pal_url` (Port von `palette_url.lua` samt
    Testvektoren), `pal_context` (Kontext aus niri ohne eigenes Panel, L4, L16), `pal_exec` (Spiegel des Drahtformats,
    `execute` mit `Host`, `Programs` als einzige Naht für Programme, `Launch`/`Exe` nur dort erzeugbar, Tests mit
    Rekorder und Fake-Programmen).
  - `src/bin/dictate-ui/`: `niri.rs` (`windows_at`, `workspaces_at`, `run_op_at` für die geschlossene Liste `NiriOp`,
    einziges `spawn_at`; `socket()` liefert in Tests keinen Socket), `pal_host.rs` (Host über niri, Kontext,
    Kurzläufer mit Frist 10 s; bis Block C `#[allow(dead_code)]` in `main.rs`).
  - Wächter: `no_shell_anywhere` enger (genau ein `Command::new(exe.path())` in `pal_host.rs`, genau ein
    `Action::Spawn` in `niri.rs`, kein `SpawnSh`, `src/ui/*.rs` startet nichts). Befund: `code()` schnitt am **ersten**
    `#[cfg(test)]` ab, mit dem neuen `cfg(test)` in `niri::socket` wäre der Rest von `niri.rs` ungeprüft geblieben;
    jetzt nur am Testmodul (Test `code_cuts_only_at_the_test_module`). Spiegeltests `palette::ui_mirror` (nur
    Bibliothek): jede Absicht und jedes Ziel des Daemons liest der Ausführer gleich, Bestätigungspflicht wie im
    Katalog, Seiten, Profile und Grenzen gleich, schlimmster Kontext besteht `Context::from_json`. **Wer `src/ui/pal_*`
    ändert, lässt auch `--lib palette::ui_mirror` laufen.**
  - Für Block C: `Programs::session()` einmal in `main`/`app` erzeugen und durchreichen (für `palette.sh` in Block D
    ein verstecktes Flag wie `--fake-keys` oder ein eigener `PATH`); beim Öffnen das fokussierte Fenster merken
    (`prior_focus`) und den Kontext über `NiriHost::context` bauen, **bevor** das Panel liegt; `execute` erst nach dem
    Schließen des Panels und in einem Arbeiter (Kurzläufer warten bis 10 s); `ExecCtx::confirmed` vom Automaten; ins
    Log nur `Intent::name` und `Outcome::kind`; Meldungen von `Outcome::Failed` ins Overlay; `Outcome::Handoff` an die
    Panels (Block D); `#[allow(dead_code)]` vor `mod pal_host` entfernen.
  - Offen für Block D (messen im verschachtelten niri): ob `FocusWindow` Arbeitsfläche und Monitor mitnimmt, ob der
    Fokus nach `Handled` sofort stimmt (L15), ob das Panel vor der Aufnahme schon fort ist.
  - Gemessen: Block B brauchte eine Sitzung (Schätzung eine bis zwei), rund 1 h (23:15–00:15), kein Bau des Injektors
    und kein verschachteltes niri nötig; Token nicht gemessen.
  - Nächster Schritt: Block C (Automat, Fenster, Taste, Stimme).
- 22.11.: **Block C, Teil C1 fertig** (Commits `e0a5666` Spezifikation und Lesarten L19–L26, `b190838` Automat). Block C
  ist in drei Teile geschnitten: **C1** Automat, **C2** Fenster, Arbeiter, Verdrahtung, Taste, **C3** Stimme, Mock, `palette.sh`.
  Je Teil ein Commit, Weitergabe an eine frische Sitzung wegen der Größe des Kontexts.
  - `ui::cmdpal` (`src/ui/cmdpal.rs`, Tests `cmdpal_tests.rs`, 24): rein, Eingaben mit `now`, Folgen als `Effect`
    (`Show`, `Hide`, `Render`, `Highlight` = Hervorhebung **und Fuß** ohne Neuaufbau, `VoiceText`, `SetInput(text)`,
    `Resolve{gen,seq,input}`, `Ask{gen,seq,input}`, `Hangup{lane,seq}` mit `Lane::{Resolve,Ask}`, `Execute{cand,
    confirmed, web_search_url, via}` **nach** `Hide`, `VoiceCancel`). Eingänge: `open(now)`, `close()`, `text_changed(text,
    now)`, `key_press/key_release`, `activate("row:N"|"confirm"|"cancel"|"close")`, `hover`, `resolve_reply(gen, seq,
    Result<&Value, CallErr>, now)`, `ask_reply(…)`, `voice_state(Option<VoicePhase>)`, `voice_partial`, `voice_level(0..1)`,
    `voice_final(text, now)`, `focus_lost`, `tick`/`next_deadline` (Leerlauf), `ask_secs(now)`, `footer()`, `verb()`.
    Fehler als `ui::diag_panel::CallErr` (Daemon-Fehler `ok:false` = `Failed(error)`).
  - Abweichung von Lua (zusätzlich zu L19–L26): nach der Vorschau der KI gilt die Totzeit (`gate.changed`; die Liste
    wechselt unter dem Finger); lokale Antworten setzen keine Totzeit (schnelles Tippen + Return bleibt möglich).
  - Geprüft: `cargo test --release --lib -- ui::cmdpal` (24), `scripts/check.sh --only fmt,clippy` grün,
    `cargo test --release --bin dictate-ui` (326) grün. Injektor in diesem Worktree gebaut (`tests/wayland/nested`).

  **Entwurf für C2** (Fenster, Arbeiter, Verdrahtung; aus der Lesung von `app.rs`, `panel.rs`, `diag.rs`, `link.rs`):
  - `src/bin/dictate-ui/pal.rs` (in `MODULES` von `primary.rs`): zwei Bahnen wie `diag.rs` (je ein Thread, Auflegen über
    einen Klon des Sockets, `Line`-Muster): `Resolve` (Frist 10 s, L3) und `Ask` (Frist `palette.timeout_ms` + 10 s;
    `send_config` in `link.rs` liest `config.palette.timeout_ms`, Vorgabe 8000, neue `Msg`). Zeile über
    `Request::PalResolve/PalAsk{input: SecretText, context: Option<SecretText>, env: SessionEnv::from_process()}.to_line()`,
    gesendete Zeile danach `wipe`; Antworten als `Wiped` an den Hauptthread (`PalMsg::{Resolved,Asked}{gen,seq,res}`).
    Aufträge mit altem `gen` gar nicht beginnen. **Beim Schließen auch das laufende `pal_resolve` auflegen** (sonst wartet die
    erste Anfrage der nächsten Öffnung bis zu 10 s dahinter; der Automat liefert dafür `Hangup{Resolve}`).
  - Ausführen: `Effect::Execute` startet einen Thread: `NiriHost::session()` (Fehler = `Outcome::Failed`),
    `ExecCtx{prior_focus, web_search_url, confirmed}`, `Programs` (einmal in `app::run`, `Arc`), `chosen` schickt
    `Request::PalChosen{intent: cand.intent.name(), target_id}` auf eigener Verbindung (Frist 2 s, Fehler nur als Art ins
    Log). Ergebnis als `PalMsg::Executed{intent: &'static str, outcome}`: Log nur `palette: <intent> <outcome.kind()>`,
    `Failed(m)` → Overlay „Command palette: <m>“ (Fehler), `Done(Some(n))` → Hinweis, `Handoff` → L26.
  - Öffnen: Taste → wie `act::capture_focus` in einem Hilfsthread `prior_focus` (fokussiertes Fenster über niri, nie das
    Panel) und `NiriHost::context(prior_focus)` holen, dann `PalMsg::Opened{prior_focus, context: Result<SecretText,_>}`
    → `cmdpal.open(now)`; Kontextfehler: ohne Kontext öffnen (`context: None`), Art ins Log. `opening` braucht dafür die
    Quelle (heute nur Textaktionen): ein Enum `Opening{Actions, Palette}`. Kontext in `App` als `SecretText` bis zum
    Schließen.
  - **Ein Panel zur Zeit:** alle Stellen in `app.rs` anfassen, die auf `panel.is_open()`/`diag.is_open()` verzweigen:
    `Msg::Lock` (Hinweis nachziehen), `actions_key`, `diagnose_key`, neues `palette_key` (offen: schließt; anderes Panel
    offen: schließt nur dieses; gesperrt: Hinweis „Command palette: the session is locked“), `ActMsg::Opened`,
    `panel_ev` (Reihenfolge diag → palette → actions), `locked` (Palette zu, Stimme ab, KI auflegen), `overlay_free`,
    Programmende (nichts zu löschen im Daemon, aber Aufnahme abbrechen).
  - Fenster: `Source::Palette` (Titel „dictate: command palette“, Breite 720) in `panel.rs`; Eingabezeile `pal_entry`
    (`gtk::Entry`, eigene Box zwischen Kopf und Scrollbereich wie `field_box`, außerhalb jedes Neuaufbaus, nur bei
    Palette sichtbar, `max_length` 1000, `set_enable_undo(false)`, `InputHints::PRIVATE`, Platzhalter „Search windows,
    apps and projects, or type a command“, Fokus nur `grab_focus_without_selecting`, in `hide()` leeren, während der
    Aufnahme `set_editable(false)`). Tasten im Capture: liegt der Fokus in `pal_entry`, gehen nur `Escape`, `Return`,
    `KP_Enter`, `Tab`, `ISO_Left_Tab`, `Up`, `Down` und Ctrl+J/M an den Automaten, alles andere an die Zeile.
    **Änderungen der Zeile ohne Text melden** (`UiEv::PalChanged`) und `entry.text()` erst beim Verarbeiten im Hauptthread
    lesen (sonst überholte ein Tippereignis im Kanal ein `SetInput` aus Verlauf oder Stimme); `SetInput` setzt mit
    blockiertem Signal (wie `set_field_text`). Ansicht in neuem `pal_view.rs` (`impl PanelUi`, in `MODULES`): Zeilen
    `row:N` (Anzeige, Detail einzeilig, Marke „Confirm“/„AI“), „AI suggestion“ über der Vorschau, Leertexte, „Asking AI“
    mit Sekunden (Sekundentakt `start_secs` → `UiEv::Tick` → `ask_secs`), Bestätigungsblock mit `cancel`/`confirm`,
    Fehlerblöcke (neutral = nicht rot), Aufnahme-Ansicht (Titel Getting ready/Listening/Transcribing, Text, Pegel). Auswahl
    mit ↑/↓ nur über `Highlight` (Hervorhebung und Fuß), nie Neuaufbau. Lage-Log `panel: layout <id> …` für Klicktests.
    Texte in `c552237:docs/w5-vorlagen/cmdpal-spec.md` Abschnitt C, Meldungen nach L25.
  - Taste: `UiCmd::Palette` (`palette`) in `ui::ctl` (Test `names_roundtrip`, Text von `answer`), `Sub::Palette` in
    `main.rs` („Open or close the command palette (Mod+Alt+Space)“), `Mod+Alt+Space { spawn "dictate-ui" "palette"; }` in
    `niri/dictate-binds.kdl` mit `niri validate -c niri/dictate-binds.kdl`; die KDL berührt auch den Schritt `gui`
    (Tabelle in CLAUDE.md). `#[allow(dead_code)]` vor `mod pal_host` entfernen. Installierte KDL nur mit Daniels Zustimmung.
  - **Früh messen** (ein kurzer Versuch im verschachtelten niri, vor `palette.sh`): dort ist Mod = Alt; ob und mit welcher
    Taste des Injektors `Mod+Alt+Space` auslöst (sonst ruft der Test `dictate-ui palette` direkt, wie die anderen Tests
    die Kürzel nutzen).

  **Entwurf für C3** (Stimme, Mock, Test):
  - Besitz der Aufnahme im Hauptthread (L23): `apply` in `app.rs` schickt `Out::Daemon(Start|Toggle)` bei offener
    Palette als `Request::PaletteVoice{toggle, app: None}` (`Commander` braucht dafür einen Weg mit Ziel; `with_app` im
    Thread sähe die `app_id` des Panels). Zustand `voice_owner: None | Pending | Palette | Discard`: `Pending` nur, wenn
    beim Senden keine Aufnahme läuft (sonst gehört sie dem Fenster); Wechsel nach Loading/Listening macht `Palette`.
    Solange `Palette`: `state` → `cmdpal.voice_state`, `partial` → `voice_partial` (nie `a.stable/tail`, Overlay bleibt
    aus außer Fehlern: in `render` den Zustand für das Overlay als Idle zeigen), `level` → `voice_level(view::level_from_db)`,
    `final` → `voice_final` (nie `queue.on_final`, auch kein `queue.recording_started` beim Start), Idle ohne `final` →
    `voice_state(None)`. `Effect::VoiceCancel` → wie Mod+Ctrl+E ohne Ausgabe (`hold.cancel()` und `apply`), Besitz
    `Discard`: ein spätes `final` wird verworfen (prüfen mit `late_final` des Mocks).
  - Mock (`tests/wayland/mock_daemon.py`): `pal_resolve` (Kandidaten je Eingabe: ein Fenster mit der echten niri-ID des
    Testfensters aus dem Kontext, eine `confirm`-Zeile `reboot`, ein Programm über ein Fake, leere Eingabe; Datei für
    „aus“ und für langsam; Log `pal_resolve <Zeichen>` ohne Text), `pal_ask` (Vorschau, verzögert, Auflegen → `pal_ask
    hung up`), `pal_chosen` (Log `pal_chosen <intent>`), `start`/`toggle` mit `target` im Log (`start_target palette`).
    Nie ein absolutes `argv[0]` auf ein echtes Programm (`Programs::find` nimmt absolute Pfade an).
  - Fake-Programme (L22): verstecktes Flag `dictate-ui run --fake-programs DIR` ersetzt den Suchpfad ganz
    (`Programs::with_dirs([DIR])`); `start_ui` in `run.sh` übergibt es immer (gilt damit auch für `restart_ui`), `run.sh`
    legt den Ordner mit Fakes an (`systemctl`, `loginctl`, `noctalia`, `pactl`, `playerctl`, `xdg-open`, `doorcam-watch`,
    `rofi`, `alacritty`, `dictate-settings`: schreiben nur ihr argv in eine Datei) und bricht ohne ihn ab.
  - `tests/wayland/palette.sh` (in `run.sh` nach `diagnose.sh`, vor `lock.sh`; Sperre der Palette in `lock.sh`): Öffnen
    (schwebend, fokussiert, Eingabezeile fokussiert ohne Markierung, `nested-clip watch-primary`), Tippen → Kandidaten,
    zuerst eine harmlose `run`-Absicht über ein Fake (argv-Datei geschrieben, `pal_chosen` im Mock-Log), dann `focus_window`
    (Fokus im verschachtelten niri), Esc schließt, `confirm` (`reboot`: erstes Return zeigt die Bestätigung, zweites
    ruft das Fake `systemctl`), Ctrl+Return fragt die KI, Esc legt auf (`pal_ask hung up`), Stimme ins Panel (Text in der
    Zeile, kein Ausführen, kein Tippen ins Fenster, Esc bricht ab, spätes `final` verworfen), ein Panel zur Zeit (Mod+I,
    Mod+Shift+I), Sperre (Panel zu, Öffnen verweigert). Kein `logout` in C. **Log-Prüfung** am Ende von `run.sh` mit
    eigenen Markerwörtern in Eingaben, `display` und Fenstertiteln (Absichtsnamen wie `reboot` stehen zu Recht im Log).
  - Nächster Schritt: C2 in frischer Sitzung, danach C3 (je Teil committen), danach Block D.
- 22.11.: **Block C, Teil C2 fertig** (Commit `1020a4b`, dazu Mock und Plan). Fenster, Arbeiter, Verdrahtung und Taste
  nach dem Entwurf oben; Abweichungen und Lesarten unten.
  - **Gemessen** (vor dem Bau, verschachteltes niri mit eigener KDL): `Mod+Alt+Space` löst dort mit dem Injektor über
    `alt+space` aus (Mod = Alt fällt zusammen), `super+alt+space` nicht; `Mod+Shift+Space` über `alt+shift+space`.
    `palette.sh` kann die echte Taste nehmen.
  - `pal.rs`: eine Bahn (`Track`, ein Thread, Auflegen über einen Klon des Sockets wie `diag.rs`) zweimal: `pal_resolve`
    (10 s) und `pal_ask` (`palette.timeout_ms` aus `get_config`, `Msg::PalAskTimeout`, + 10 s). **Jeder Auftrag
    antwortet genau einmal**, auch übersprungene (altes `gen`, vorher aufgelegt: `Cancelled`); sonst nähme der Automat
    nach einer fehlenden Antwort keine Eingabe mehr an. Öffnen im Hilfsthread (`NiriHost::opened`: Fokus und Kontext aus
    **einer** Fensterliste, Fokus nie das eigene Panel, auch nicht in `ExecCtx`), Ausführen im Thread mit `pal_chosen`
    auf eigener Verbindung (2 s, ins Log nur Absicht und Art des Fehlers). 7 Tests mit Fake-Daemon (Auflegen beim
    Blockieren und vor dem Beginn, altes `gen`, Frist, ohne Daemon, `pal_chosen`), 1 Test für `opened`.
  - `panel.rs`: `Source::Palette` (720), Eingabezeile `pal_entry` in eigener Box (Platzhalter, `max_length` 1000, ohne
    Undo, `InputHints::PRIVATE`, Fokus nur `grab_focus_without_selecting`, in `hide()` geleert und wieder editierbar,
    Sichtbarkeit in `show()` vor `present`), eigene Tastenliste `PAL_KEYS` (Esc, Return, KP_Enter, Tab,
    ISO_Left_Tab, Up, Down, Ctrl+J/M; `FIELD_KEYS` der Diagnose unverändert), Loslassen geht immer an den Automaten,
    `UiEv::PalChanged` ohne Text (gelesen beim Verarbeiten). `pal_view.rs`: Vorrang Aufnahme → Warten → Fehler → Liste,
    Zeilen `row:N` einzeilig (`one_space`, „…“), Marke „Confirm“/„AI“, „AI SUGGESTION“ mit `→` und voller Adresse bei
    `open_url`, Leertexte, Bestätigung ersetzt den Fuß (Knöpfe `cancel`/`confirm`), Fuß aus `CmdPal::footer` ohne
    Neuaufbau bei `Highlight`, Lage-Log mit `input`.
  - `app.rs`: `Opening{Actions, Palette}` (jedes Öffnen verbraucht nur sein eigenes; ein weiteres Kürzel oder die Sperre
    dazwischen öffnet keins, der Kontext fällt ungenutzt), ein Panel zur Zeit an allen Stellen (Kürzel, `Opened`,
    `panel_ev` Diagnose → Palette → Textaktionen, `overlay_free`, Hinweis bei abgeschalteter Sperr-Erkennung), Sperre
    schließt die Palette (legt auf), Leerlauf über eigenen Zeitgeber, Kontext als `SecretText` bis `Hide`.
    `Programs::session()` einmal in `main` und als Parameter an `app::run` (C3 ergänzt nur das Flag).
  - Mock: minimales `pal_resolve` (erstes Fenster des Kontexts mit echter ID, bei Eingabe eine `confirm`-Zeile, Log
    nur Zahlen). Rauchtest im verschachtelten niri **17 von 17 grün** (öffnet schwebend und fokussiert, leere Eingabe
    fragt, Kontext trägt das Zielfenster, Tippen → 9 Zeichen beim Mock und zwei Zeilen, kein PRIMARY beim Fokussieren
    und Tippen, Fokus beim Öffnen ist das Zielfenster, Esc schließt und gibt den Fokus zurück, Mod+I schließt nur die
    Palette, Mod+Alt+Space schließt sie wieder, kein Testtext im Log); Bilder geprüft (Zeile, Auswahl, Fuß wechselt mit
    ↓ auf „Run…“/„Ctrl+Enter Ask AI“). Skript als Vorlage: `f660676:docs/w5-vorlagen/palette-smoke.sh` (in C3b durch
    `tests/wayland/palette.sh` ersetzt und gelöscht).
  - Geprüft: `scripts/check.sh --only fmt,clippy` grün, `cargo test --release --bin dictate-ui` (335) und `--lib --
    ui::ctl ui::cmdpal` (26) grün, `niri validate -c niri/dictate-binds.kdl` gültig, Schritt `gui` grün (202 s; die KDL, Schweben von
    `dictate-settings`).
  - Lesarten aus C2 (bei Widerspruch korrigieren):
    - **L27** Fuß: die Regel „reicht die Breite nicht, entfällt `Mod+E Hold to speak`“ ist nicht gebaut. Bei 720 passen
      alle fünf Paare (Bild mit ↑↓, Enter, Ctrl+Enter, Mod+E, Esc: rund drei Viertel der Breite); erst bei sehr großer
      Schrift würde der Fuß breiter als das Panel.
    - **L28** Ergebnis der Ausführung im Overlay: `Failed(m)` als Fehler „Command palette: m“, `Done(Some(n))` als
      Hinweis nur mit `n` (z. B. „The volume is already at 100%“), `Handoff` nach L26.
    - **L29** Zwei Kürzel kurz nacheinander (das zweite, während das erste noch Fokus und Kontext holt) öffnen keins, wie
      bisher bei Mod+I und Mod+Shift+I.
    - **L30** Ohne Kontext von niri (Fehler) öffnet die Palette trotzdem (ohne Fenster und Arbeitsflächen, ins Log nur
      „no context from niri“); scheitert niri beim Ausführen, meldet das Overlay „Command palette: niri is not reachable“.
    - **L31** Texte über die Spezifikation hinaus: bei abgeschalteter Sperr-Erkennung die Zeile „Lock detection is off:
      the palette stays open when the screen locks.“ (wie bei den anderen Panels); in der Aufnahme-Ansicht unter dem Pegel
      „Speak your command. Esc cancels.“ (C3 gleicht sie mit Abschnitt B5 der Spezifikation ab). Die Linie zwischen den
      Zeilen der Liste (Spezifikation C) ist nicht gezeichnet (Zeilen wie bei den anderen Panels mit Hervorhebung);
      offen, ob sie fehlt. (Entschieden 22.11., Entscheidung 17: ohne Linie.)
  - Nicht belegt in C2: der Weg `Hide` → Ausführen im Thread → `Executed`/`pal_chosen` lief nur in den Unit-Tests von
    `pal_exec`/`pal.rs`, nie im verschachtelten niri (kein Return auf einem Kandidaten ohne `--fake-programs`);
    `wayland` und `unit` ganz liefen nicht, `palette::ui_mirror` nicht nötig (kein `src/ui/pal_*` geändert).
  - Offen für C3: Stimme (`Effect::VoiceCancel` ist bis dahin ohne Wirkung, die Palette besitzt noch keine Aufnahme;
    Mod+E bei offener Palette diktiert wie bisher, die Ausgabe ins Panel sperrt `panel_visible`), `--fake-programs`,
    Mock `pal_ask`/`pal_chosen`/aus/langsam, `palette.sh` aus der Vorlage, Sperre der Palette in `lock.sh`, Log-Prüfung
    mit den Testtexten in `run.sh`. Für später (kein Teil von W5): `diag.rs` könnte die Bahn `Track` aus `pal.rs`
    übernehmen (gleiche Auflege-Logik zweimal).
  - Gemessen (Zeitstempel der Dateien der Sitzung): C2 brauchte eine Sitzung, rund 25 min (00:42–01:07); Bau und
    gezielte Tests bis 00:58, zwei Rauchtestläufe bis 01:01, danach `gui` 202 s. Token nicht gemessen.
  - Nächster Schritt: C3 in frischer Sitzung (Stimme, Mock, `palette.sh`), danach Block D.
- 22.11.: **Block C, Teil C3a fertig** (Commits `00c495c` Fakes, `0629ad6` Stimme). C3 ist wegen der Größe des Kontexts
  zweigeteilt: **C3a** Fakes und Stimme (Rust), **C3b** Mock, `palette.sh`, Sperre, Log-Prüfung, `wayland` ganz.
  - **Grundlauf** vor jeder Änderung: `scripts/check.sh --only wayland` auf `ecbedc7` grün (281 ok, 212 s); C2 hat die
    anderen Panels nicht zurückgeworfen.
  - **Fakes (L22):** `dictate-ui run --fake-programs DIR` (versteckt) ersetzt den Suchpfad ganz; ein relativer, fehlender
    oder Nicht-UTF-8-Ordner beendet mit Fehler (nie Rückfall auf `PATH`; `Programs::with_dirs` verwürfe ihn still), Test
    `fake_programs_must_be_an_absolute_directory` in `main.rs`; Log `palette programs only from DIR (test mode)`.
    `run.sh` legt `$D/fakes` an (`systemctl`, `loginctl`, `noctalia`, `pactl`, `playerctl`, `xdg-open`, `doorcam-watch`,
    `rofi`, `alacritty`, `dictate-settings`, `dictate-fake-app`; jedes schreibt nur `name [arg]…` als Zeile nach
    `$D/fake-programs.log`, Pfad fest eingetragen, weil über niri `Spawn` gestartete Fakes die Umgebung von niri haben),
    bricht ohne sie ab; `start_ui` übergibt das Flag immer und scheitert ohne die Log-Zeile (gilt für `restart_ui` und
    die Neustarts in `lock.sh`), ebenso die zweite Instanz; `diag-shots.sh` mit leerem Ordner.
  - **Stimme:** reiner Automat `ui::voice_owner` (9 Tests): `claim` (Mod+E/Mod+Shift+E bei offener Palette und Client
    `idle`), `sent`/`failed(seq)`, `state(recording, finalizing, open)` legt den Besitz **je Sitzung** beim Wechsel nach
    `loading`/`listening` fest (`Route::{Window, Palette, Discard}`), `final_event`, `cancel`, `hides_overlay`, `reset`.
    `link::Commander::send_palette(toggle)` schickt `Request::PaletteVoice{toggle, app: None}` (gleiche laufende Nummer
    wie `send`). `app.rs`: Route `Palette` → `cmdpal.voice_state/partial/level/final`, nie `queue.recording_started`,
    keine Eingabemethode (`arm`), Overlay als `idle` (nur Hinweise und Fehler); Route `Discard` → nirgends;
    `Effect::VoiceCancel` und `Effect::Hide` → `voice.cancel()` und dann `hold.cancel()` (nach dem Block, `apply` leiht
    selbst), Mod+Ctrl+E ebenso samt `voice_state(None)`, `Msg::Failed` für Start/Toggle → `failed(seq)`, `Disconnected`
    → `reset`. Mitschrift und `final` der Palette werden nach dem Weiterreichen überschrieben (`wipe`), ins Log nur
    Zeichenzahl und Besitzer (`recording for the command palette`, `final: N chars for the command palette`, `… of a
    cancelled palette recording, discarded`).
  - **L31 mit B5 abgeglichen:** die Aufnahme-Ansicht zeigt leer den Hinweis an Stelle des Texts („Speak your command. Esc
    cancels.“, beim Umwandeln „Esc cancels.“), sonst `stable` + Leerzeichen + grau `tail`, einzeilig und vorn gekürzt; die
    feste Hinweiszeile unter dem Pegel entfällt. Die Linie zwischen den Zeilen bleibt offen (L31).
  - Lesarten aus C3a (bei Widerspruch korrigieren):
    - **L32** Die Palette beansprucht eine Aufnahme nur, wenn der Client den Daemon `idle` sieht. Läuft oder endet gerade
      eine Aufnahme eines Fensters, bleibt Mod+E wie bisher (Halten/Umschalten des Fensters): `cancel` während
      `finalizing` bräche im Daemon das `final` des Fensters ab (`daemon.rs` 734), und ein `start` dann würde dort als
      nahtlose nächste Sitzung gemerkt.
    - **L33** Statt des einen Zustands `None | Pending | Palette | Discard` aus dem Entwurf zwei: Besitz der nächsten,
      noch nicht begonnenen Sitzung (`None | Palette | Orphan`) und der laufenden (`Window | Palette | Discard`). Grund: ein
      Abbruch vor dem ersten `loading` muss die dann beginnende Sitzung verwerfen, ein „verwerfen“ darf aber nie über den
      Beginn der nächsten Sitzung hinaus gelten (sonst verschluckte es ein Fensterdiktat, wenn ein `final` fehlt).
      `idle` ohne `final` macht aus `Palette` `Discard` (B5).
  - Geprüft: `scripts/check.sh --only fmt,clippy` grün, `cargo test --release --bin dictate-ui` (345) und `--lib --
    ui::voice_owner ui::cmdpal ui::hold` (46) grün; `scripts/check.sh --only wayland` auf C3a grün (281 ok, 316 s; jede
    Instanz mit `palette programs only from …`, 10 Aufnahmen „for the window“, `fake-programs.log` leer: kein Rückschritt
    im Diktat). **Nicht gelaufen:** die Stimme ins Panel im verschachtelten niri (der Mock kennt `target` noch nicht).
  - **Offen für C3b** (Entwurf für C3 oben, dazu):
    - Mock: `reboot` nur bei einem bestimmten Eingabewort statt bei jeder Eingabe; nur bloße Programmnamen in `argv`
      (`Programs::find` nimmt absolute Pfade an; `/usr/bin/env` ist echt, aber harmlos); ein `run`-Kandidat
      `launch_app` mit `argv ["dictate-fake-app", …]` über das Fake; `pal_ask` (Vorschau, Verzögerung per Datei,
      Auflegen → `pal_ask hung up`: „hung up“ loggt der Mock bisher nur für `diag_*`), `pal_chosen <intent>`, Datei für
      „aus“ und „langsam“, `start`/`toggle` mit `target` im Log (`start_target palette`).
    - Texte nie ins Log: Prüfung über Zeichenzahlen (nach der Stimme `pal_resolve <Länge von FINAL>`, nach Esc mit
      `late_final` nie `<Länge von LATE>`); nach der abgebrochenen Palettenaufnahme ausdrücklich ein Fensterdiktat ins
      Zielfenster prüfen (Route `Window` kehrt zurück; `lock.sh` diktiert nicht mehr).
    - Die heiklen Abläufe (nahtloses `start` während `finalizing`, fehlendes `final`) kann der Mock nicht nachbilden
      (er startet nur aus `idle`): sie sind nur in `ui::voice_owner` getestet.
    - Helfer aus `actions.sh`/`diagnose.sh` (`key`, `panel_json`, `panel_up`, `settle`, `focus`, `acts`) nutzen, nicht
      aus der Vorlage doppeln; der Wächter von PRIMARY unter eigenem Namen (`nested_spawn` leert das Log je Name) und per
      PID beenden.
    - „Ein Panel zur Zeit“ nach benutzter Palette: bleibt `FOCUS_WITHIN` an der verborgenen `pal_entry` hängen, die
      Weiche in `panel.rs` zusätzlich über `is_mapped()` sperren.
  - Gemessen: C3a rund 20 min Bau (01:08–01:25 laut Logs und Commits, dazu 212 s Grundlauf), Token nicht gemessen.
  - Nächster Schritt: C3b in frischer Sitzung, danach Block D.
- 22.11.: **Block C fertig** (Teil C3b: Commits `b8fdb61` Korrektur im Panel, dort auch die Löschung von
  `f660676:docs/w5-vorlagen/palette-smoke.sh`; `870609d` Mock und Tests; Nachzug nach der Durchsicht siehe unten). Mock,
  `tests/wayland/palette.sh`, Sperre der Palette in `lock.sh`, Log-Prüfung.
  - **Mock** (`tests/wayland/mock_daemon.py`, Modulkopf): Kandidaten je Eingabewort (`PAL_WORDS`, nur Tasten gleich unter
    `de`/`us`, keins in FINAL/LATE/PARTIAL): leer = das Testfenster der Palette (`app_id` `dictate-test-pal`, echte
    niri-ID aus dem Kontext), `fakeapp` = `launch_app` mit `argv ["dictate-fake-app", "--from-palette"]` (nur bloße
    Namen), `reboot` = `confirm`; beide Wörter `confident` (sonst fragt Return die KI, `enter_asks`). `pal_ask` mit
    Vorschlag (Klasse `run` wie im Katalog: `ai` tragen nur `ask`, `text_action`, `explain_clipboard`, `diagnose`, und
    der Fuß zeigt für sie „Enter Ask“), Datei `pal_slow_ask` (10 s, erkennt das Auflegen über `select`/`MSG_PEEK` →
    `pal_ask hung up`), `pal_off`, `pal_slow` (1 s), `pal_chosen <intent>`, `start_target <target>` bei `start`/`toggle`.
    `target.py` nimmt einen Titel aus `TARGET_TITLE` (Testtext nur im Titel, nie in der `app_id`, die zu Recht im Log steht).
  - **`palette.sh`** (in `run.sh` nach `diagnose.sh`, vor `lock.sh`; Helfer aus `output.sh`/`actions.sh`/`diagnose.sh`, eigene
    `pal_open`, `pal_type`, `pal_close`; Zähler über das verankerte `count`): P1 Öffnen (schwebend, fokussiert, Tippen in
    der Zeile, kein PRIMARY, Esc gibt den Fokus zurück), P2 Return startet das Fake über niri `Spawn` (`pal_chosen
    launch_app`, kein `pal_ask`), P3 Return auf dem Fenster fokussiert es, P4 `confirm` (erstes Return zeigt die
    Bestätigung, Esc hebt nur sie auf, Return und Return → Fake `systemctl [reboot]`), P5 Return vor der Antwort (langsamer
    Mock) läuft nach ihr, P6 aus (Ansicht `Error`, neutral im Bild), P7 Ctrl+Return fragt, Esc legt auf (Panel bleibt),
    der Vorschlag läuft mit Return, P8 Stimme ins Panel (`start_target palette`, Ansicht `Voice`, Overlay aus, `final: 47
    chars for the command palette` → `pal_resolve 47`, nichts ausgeführt, nichts ins Fenster getippt), P9 Esc bricht ab,
    spätes `final` verworfen (nie `pal_resolve 33`, kein `stop` beim Loslassen), P10 Fensterdiktat danach ins Zielfenster,
    P11 ein Panel zur Zeit nach benutzter Palette (Buchstabe an die Textaktionen, Ziffer und Return an die Diagnose, Tab
    und Buchstaben ins Feld „What is wrong?“ → `diag_symptom 3`), P12 kein Fake läuft mehr (`pgrep` mit `[/]`-Muster,
    trifft sich nicht selbst), P13 ein Panel zur Zeit über die Kürzel (Mod+Alt+Space schließt die offene Palette; Mod+I und
    Mod+Shift+I schließen die Palette nur, Mod+Alt+Space die Textaktionen bzw. die Diagnose nur, je mit Log-Zeile und ohne
    zweites Panel; die Vorlage des Rauchtests prüfte zwei davon). Vor P1: das Fake-Log ist leer (Diktat, Ausgabe,
    Textaktionen und Diagnose starten nie ein Programm der Palette). Bilder `palette-{typed,confirm,off,ai,voice}.png`
    geprüft.
  - **`lock.sh`**: 28c Sperre bei offener Palette (zu, `act_clear`/`diag_clear`), mitten in `pal_ask` (aufgelegt), während
    einer Aufnahme der Palette (`cancel`, kein `stop` beim Loslassen; danach gehört das Diktat wieder dem Fenster und
    landet dort), 31b Mod+Alt+Space bei Sperre verweigert (Log, Hinweis im Overlay, kein `pal_resolve`), nach dem
    Entsperren wieder offen, 33 ohne System-Bus öffnet die Palette mit Hinweis (neue Debug-Zeile `palette: lock detection
    off notice shown`).
  - **Log-Prüfung** in `run.sh`: `palmarker` (Eingabe, Anzeige, Fenstertitel), `fakeapp`, `from-palette` (argv),
    „Fenster der Palette“ (Titel) nie in `ui*.log`.
  - **Befund und Korrektur** (`b8fdb61`): nach benutzter Palette erreichten Buchstaben die Textaktionen nicht und Return die
    Diagnose nicht (P11 rot, belegt im ersten Lauf): GTK behielt den Fokus an der verborgenen `pal_entry` (`FOCUS_WITHIN`),
    die Weiche gab Buchstaben an die unsichtbare Zeile. `PanelUi::hide` setzt den Fokus des Fensters zurück, die Weiche
    zählt nur ein gemapptes Feld (`typing_in`: `is_mapped()` und `FOCUS_WITHIN`, auch für das Feld der Diagnose).
  - Geprüft: `scripts/check.sh --only fmt,clippy` grün, `cargo test --release --bin dictate-ui` (345) grün, `scripts/check.sh
    --only wayland` grün (360 ok, 270 s, Last 5–7, `logs/check/20261122-014941`), nach dem Nachzug der Durchsicht (P13,
    Fake-Log, KI-Verzögerung 10 s statt 3 s gegen Wackeln unter Last) **374 ok** in 308 s (Last 9–13,
    `logs/check/20261122-020051`); `palette::ui_mirror` nicht nötig (kein
    `src/ui/pal_*` geändert). Vorher fünf Läufe eines Wrappers (Kopf von `run.sh`, Helfer, `palette.sh`, `lock.sh`) im
    Scratchpad.
  - Nicht belegt: nahtloses `start` während `finalizing` und fehlendes `final` (Mock startet nur aus `idle`; nur in
    `ui::voice_owner` getestet). Offen bleibt L31 (Linie zwischen den Zeilen).
  - Gemessen: C3b rund 30 min (Aufräumen und Lesen bis ~01:38, Bau und vier Wrapper-Läufe bis 01:49, `wayland` 270 s),
    Token nicht gemessen.
  - **Wrapper zum Iterieren** (nicht im Repo, je Sitzung neu im Scratchpad): Kopf von `run.sh` bis vor Abschnitt 1
    (`HERE` absolut, `LOGS` auf einen eigenen Ordner), dann die erste Oberfläche beenden (`kill -TERM "$UI_PID"`, warten),
    dann die Helferblöcke ohne Tests: `output.sh` bis vor dem ersten `nested_spawn` (Zeile 8–98), `actions.sh` bis vor
    „# --- 21.“ (7–134, startet die Oberfläche mit `paste` neu), `diagnose.sh` bis vor „# Frische Oberfläche“ (12–58),
    danach `source palette.sh` (und `lock.sh`), Log-Prüfung, Summe. Zeilenbereiche vor dem Bau neu bestimmen. Ein Lauf
    dauerte rund 2–3 min statt 5 min für `wayland` ganz.
  - Nächster Schritt: Block D in frischer Sitzung (Zuschnitt oben: Übergänge `question`/`clipboard`/`preselect`,
    CLIPBOARD-Leser L8, Diagnose mit Vorauswahl, `palette.sh` vollständig für jede Absicht samt niri-Aktionen im
    verschachtelten niri; die Helfer und das Mock-Muster aus C3b weiterverwenden).

- 22.11.: **Block D, Teil D1 fertig** (Commits `5ded682` Rust, `df6970e` Mock und Tests). D ist zweigeteilt: **D1**
  Übergaben an die anderen Panels samt CLIPBOARD-Leser und ihre Tests, **D2** jede übrige Absicht in `palette.sh`.
  - **`ui::panel`** kennt die Quelle (`Source` aus `actions::store`): `toggle` (Mod+I) wie bisher mit der Markierung,
    `open_with(source, preselect, question, target)` für die Palette (ein offenes Panel schließt vorher). `question`:
    die Frage liegt im Automaten (beim Schließen überschrieben), die eine Aktion `ask` startet nach `act_grab` sofort,
    Back und Esc schließen, Fehler ohne Back; `clipboard`: Vorschau, die eine Aktion ist hervorgehoben, gesendet wird
    erst auf ein neues Return, ihre Taste oder Klick (Esc sendet nichts); `selection` mit `preselect`: die vorgewählte
    Aktion startet, sobald erlaubt (ein Geheimnis lässt den Sperrblock stehen, eine unbekannte Aktion zeigt einen
    Fehler). `Effect::Grab` trägt die Quelle, `Effect::Chosen` meldet einmal je Öffnung aus der Palette das erste
    Ergebnis. Texte je Quelle (Titel „Ask“/„Explain clipboard“, „Your question, N characters“, „from the clipboard“,
    Frage als Zeile über der Antwort, Anleitungen mit Mod+Alt+Space, Sperrblock je Quelle). 6 neue Unit-Tests.
  - **Diagnose:** `DiagPanel::open_from_palette(preset)` (Vorbelegung über das vorhandene `preset_of`), `Effect::Chosen`
    einmal, sobald die Profile da sind; Diagnose aus = keine Wahl. Öffnen aus der Palette beendet den Boot-Hinweis wie
    Mod+Shift+I. Unit-Test.
  - **CLIPBOARD-Leser (L8):** `clip::read_clipboard_text` (in `clip.rs`, weil `sources_stay_apart` die Zwischenablage
    dort festhält): Grenzen und Frist wie PRIMARY (`actions::selection`), Kodierung aus `primary.rs` (`decode`,
    `TEXT_TYPES` jetzt `pub`), Hinweis `secret` → nur er wird gelesen, Meldung „The clipboard holds a password“
    (`ui::panel::CLIPBOARD_PASSWORD`, neutral); gehört die Zwischenablage einer eigenen Quelle (zurückgelegt, Copy),
    kommt der Text aus dem Speicher; Wechsel beim Lesen = Fehler. `Cmd::ReadClipboard` im Ausgabe-Thread (mit
    Anspruch wie `ReadPrimary`, auch in `refuse`, Test `clipboard_read_given_up_is_skipped`), `OutTx::read_clipboard`
    nur aus `act::grab` für die Quelle `clipboard`; `sources_stay_apart` prüft das (genau ein Aufrufer je Stufe).
  - **`act.rs`:** Greifen je Quelle (PRIMARY, CLIPBOARD, Frage aus dem Panel), `app` nur für die Markierung, Log
    `act_grab (<quelle>): N chars …`.
  - **`app.rs`:** `Outcome::Handoff` öffnet im selben Fenster die Quelle der Textaktionen bzw. die Diagnose
    (`pal_handoff`); dieselben Sperren wie ein Kürzel (Sperre, ein Öffnen oder Panel dazwischen: verfällt ohne
    `pal_chosen`). Ziel für `text_action` ist das Fenster vor dem Öffnen der Palette samt `app_id` (`NiriHost::opened`
    liefert jetzt `Focus = (id, app_id)`, durch `PalIo::execute` bis `PalMsg::Executed { origin }` gereicht; `Origin`
    ohne `Debug`). `pal_chosen` danach über `PalIo::chosen_later` in einem Hilfsthread; Absicht und Ziel-ID liegen in
    `act_origin`/`diag_origin` und verfallen, sobald das Panel zu ist. `HANDOFF_LATER` und L26 entfallen.
  - **Mock und `palette.sh`:** Wörter `question` (`ask`), `clip` (`explain_clipboard`), `diagunit` (`diagnose`, Profil
    `unit`, Suchbegriff `backup`), alle Klasse `ai` und `confident`; `pal_ask` auf `polish` schlägt `text_action`
    `rephrase` vor. `act_grab` je Quelle (`grab_<quelle> N app`, damit `acts act_grab` nur Markierungen zählt), Datei
    `diag_off`, `diag_plan_profile <p>`. Tests P14 `ask` (Frage exakt beim Daemon, Antwort ohne Auswahl, `pal_chosen
    ask` danach), P15 `explain_clipboard` (nichts liest CLIPBOARD vor der Absicht, Inhalt exakt, **ein gehaltenes
    Return 1,5 s lang sendet nichts**, auch nicht nach dem Loslassen, erst ein neues; Esc in der Vorschau sendet
    nichts), P16 Passwort in CLIPBOARD (Meldung, nichts an den Daemon, nur der Hinweis gelesen), P17 `text_action` über
    die KI (PRIMARY exakt samt `app_id`, `rephrase` läuft, Replace angeboten, `pal_chosen text_action`), P18
    `diagnose` (Profil `unit` vorgewählt, `backup.service` gewählt, `pal_chosen diagnose`; aus: keine Wahl). Log-Prüfung
    in `run.sh` um Frage, CLIPBOARD-Text, Passwort, die Eingabewörter und den Suchbegriff `backup` (nachgeprüft an den
    Logs von `20261122-023422`). Bilder `palette-{ask,clipboard,password,
    diagnose}.png` geprüft (das Ask-Bild erst 1 s nach dem Ergebnis: das Wachsen über niri läuft dem letzten Bild von GTK
    nach).
  - Geprüft: `scripts/check.sh --only fmt,clippy` grün, `cargo test --release --bin dictate-ui` (352) grün, `--lib
    ui::panel ui::diag_panel ui::gate` (47) grün, `scripts/check.sh --only wayland` grün (**410 ok**, 405 s, Last 8–10,
    `logs/check/20261122-023422`); vorher zwei Läufe des Wrappers (117 ok). `palette::ui_mirror` nicht nötig (kein
    `src/ui/pal_*` geändert), `unit` ganz nicht gelaufen (Block E).
  - Lesarten aus D1 (bei Widerspruch korrigieren):
    - **L34** `pal_chosen` einer Übergabe wie `on_success` in Lua: bei den Textaktionen mit dem ersten Ergebnis, bei der
      Diagnose, sobald die Profile da sind (nicht schon beim Öffnen, wie der Zuschnitt oben sagte). Fehler, Abbruch,
      Sperrblock und ausgeschaltete Funktion melden keine Wahl.
    - **L35** Gehaltenes Return über die Übergabe: GTK wiederholt unter Wayland nur Tasten, deren Druck die Fläche sah;
      nach dem Verbergen der Palette kommt keine Wiederholung im neuen Panel an (gemessen in P15; dass der Injektor im
      verschachtelten niri überhaupt wiederholt, belegt die Gegenprobe P15a aus D2). Die Totzeit gilt
      zusätzlich. Ein Übertragen der gedrückten Tasten in den neuen `Gate` ist darum nicht gebaut (es hätte bei einem
      ungesehenen Loslassen das nächste echte Return verschluckt).
    - **L36** Die Zwischenablage zeigt die Herkunft „from the clipboard“ statt einer `app_id`, und `act_grab` der Palette
      geht ohne `app` (Replace gibt es dort nicht; `replace_ok` setzt der Daemon nur für die Markierung).
    - **L37** Eine abgeschnittene Typenliste (`Offer::truncated`) lehnt der CLIPBOARD-Leser nicht ab: der Hinweis
      `secret` wird laut `wl.rs` immer gemerkt (wie bei `read_primary`).
  - Gemessen: D1 rund 65 min (Aufräumen und Lesen bis ~01:55, Rust und Unit-Tests bis 02:20, Mock, Tests und zwei
    Wrapper-Läufe bis 02:33, `wayland` 405 s), Token nicht gemessen.
  - Offen bleibt L31 (Linie zwischen den Zeilen). Nicht belegt: Übergabe während einer Sperre oder eines anderen
    Kürzels (nur im Code, `pal_handoff`), `ask` mit Geheimnis in der Cloud (nur Unit-Test).
  - Nächster Schritt: **D2** in frischer Sitzung: `palette.sh` für jede übrige Absicht gegen Mock und Fakes
    (`view_workspace`, `move_window_to_workspace`, `previous_workspace`, `column_display`, `toggle_window`,
    `close_window`, `open_project`, `open_folder`, `open_url`, `web_search`, `lock`, `volume`, `media`,
    `show_widget`, `dictate_settings`, `screenshot`, `logout`, `poweroff`, `suspend`), niri-Aktionen im verschachtelten
    niri (Fokus über Arbeitsflächen, ob niri bei `FocusWindow` Arbeitsfläche mitnimmt, ob `FocusedWindow` sofort nach
    `Handled` stimmt (L15), Screenshot ohne Panel), Log-Prüfung um die neuen Wörter; dann `wayland` ganz und Eintrag
    „Block D fertig“. Hinweise für D2: `logout` schickt `Quit` und beendet das verschachtelte niri (eigene
    Harness-Instanz oder als letzter Test nach `lock.sh`); vor jedem Screenshot die Config des verschachtelten niri auf
    `screenshot-path` prüfen (Vorgabe schriebe nach `~/Pictures` und setzte die Zwischenablage); ein Ausgang zeigt kein
    Verhalten über mehrere Monitore (als nicht belegt melden, nicht entscheiden).

- 22.11.: **Block D fertig** (Teil D2: Commits `7d73515` Nachträge zu D1, `09ed03a` Programme und System, `922a8f1` niri-Aktionen,
  Screenshots und Abmelden, `395dfcc` Rennen in P23; nur Tests, kein Rust geändert).
  - **Nachträge zu D1:** P15a Gegenprobe zu L35: eine gehaltene Taste des Injektors wiederholt im verschachtelten niri
    (`down:a`, 1,5 s, `up:a` → Eingabe mit 7–8 Zeichen; langsamer als `repeat-rate 35`, vermutlich GTK unter pixman) —
    **L35 gilt wie geschrieben**, P15 beweist damit etwas, keine Frage an Daniel. P16b: nach „Copy“ der Textaktionen kommt
    CLIPBOARD aus der eigenen Quelle in 452–631 ms (Frist 2 s, Grenze im Test 1 s) und exakt gleich dem Ergebnis (`st.restored`-Zweig von
    `read_clipboard_text` jetzt belegt). P15 prüft zusätzlich nach `pal_type clip` vor Return, dass nichts CLIPBOARD las.
    P13 zählt `diag_plan` verankert (`count`, nicht `acts`: das traf `diag_plan_profile` mit).
  - **Mock:** Wörter `qq…` in `PAL_CMDS` je Absicht bzw. Wert (Absicht, Ziel, Klasse wie im Katalog; `qq` steht in keiner
    legitimen Logzeile, die Log-Prüfung nimmt die ganzen Wörter, weil `$FAKES` den Zufallsnamen von mktemp trägt). Ziele
    nur aus dem Kontext (Fenster über `app_id` `dictate-test-ws`/`dictate-test-close`, Arbeitsfläche über `idx`), ohne
    Ziel kein Kandidat (`pal_target_missing <absicht>`), nie ein Ersatzfenster. Fakes neu: `claude`; `pactl
    get-sink-volume` liest `$D/pactl-volume`; `systemctl suspend` scheitert mit der Meldung von systemd wie auf blub.
  - **`palette.sh` P19–P27** (Helfer `fake_n` = ganze feste Zeile, `pal_run WORT [confirm]`, `pal_expect`): P19 `open_project`
    (`alacritty --working-directory <dir> -e <claude>`), `open_folder`, `open_url`, `web_search` (Begriff mit Leerzeichen,
    `&`, `#`, Umlauten als **ein** kodiertes argv-Element), `show_widget` doorcam und emoji, `dictate_settings --page`;
    P20 `lock`, `show_widget` clipboard_history, `media`, `volume` (set, mute, +10 bei 95 % → `+5%` nach dem Lesen, bei
    100 % nichts gesetzt und Hinweis im Overlay); P21 `poweroff` erst nach dem zweiten Return, `suspend` scheitert (`failed`,
    Meldung im Overlay, Bild `palette-suspend.png`, kein `pal_chosen`); P22 Fokus über Arbeitsflächen, `view_workspace`,
    `previous_workspace`; P23 `column_display` (L15); P24 `move_window_to_workspace` (Fokus bleibt); P25 floating,
    fullscreen, maximized auf ein nicht fokussiertes Fenster, je hin und zurück; P26 `close_window` (Fenster bis zum
    zweiten Return da, danach fort; ohne Fenster kein Kandidat, nichts geschlossen); P27 Screenshots.
  - **`logout.sh`** (eigene Datei, in `run.sh` nach `lock.sh`, P28): erstes Return fragt nur (niri läuft, keine Wahl), das
    zweite beendet das verschachtelte niri, `pal_chosen logout` kam vorher an (L14 gemessen); `dictate-ui` endet mit der
    Sitzung; Kinder von niri sammelt der Test selbst ein, danach kein `nmsg` mehr. Abbau ohne Reste.
  - **Gemessen im verschachtelten niri** (ein Ausgang):
    - `FocusWindow` auf ein Fenster einer anderen Arbeitsfläche nimmt die Arbeitsfläche mit (Fokus und Arbeitsfläche 2).
    - **L15:** `FocusedWindow` meldet den neuen Fokus sofort nach `Handled` in **6 von 6** Aufrufen je Lauf (36 von 36 in sechs Läufen), das Ziel lag jeweils auf
      einer anderen Arbeitsfläche und war vor dem Öffnen nicht fokussiert. Wirkung sichtbar: zwei Fenster in einer Spalte,
      Kachelhöhe 432 (normal) → 875 (tabbed) → 432.
    - `previous_workspace` nach `view_workspace` führt zurück; Öffnen und Schließen der Palette zählen nicht als Wechsel.
    - **Screenshot ohne Panel:** Gegenprobe mit offenem Panel (`niri msg action screenshot-screen`) dunkel in dessen Mitte
      (Helligkeit 28), über die Palette hell (244); `screenshot window` hat die Größe des Fensters vor dem Öffnen;
      `area` öffnet niris Auswahl ohne Panel, nichts in der Zwischenablage vor dem Bestätigen, Return nimmt auf, danach
      kommen Tasten wieder an. Messpunkt im linken Viertel des Panels (die Mitte trifft den Mauszeiger, Helligkeit 207).
      Vor jeder Aufnahme prüft der Test `screenshot-path null` in der Config des
      verschachtelten niri; Bilder nur in dessen Zwischenablage, nichts Neues unter `~/Pictures`.
    - **Mit Animationen** (Wrapper mit `NESTED_ANIM` = `animations { }`, Vorgaben von niri; die gesicherte Config belegt
      es): `screenshot full` über die Palette 8 von 8 ohne Panel. Gegenprobe Esc und sofort `niri msg action
      screenshot-screen`: mit Animationen 2 von 8 **mit** Panel (voll deckend, Helligkeit 28), ohne Animationen 0 von 5.
      Der Ausführer wartet nicht auf das Verschwinden des Panels (`pal.rs` `execute` nach `Hide`); die Palette gewinnt das
      Rennen bisher durch ihren Weg (Arbeiter, niri-IPC), belegt ist das nicht. Frage F1 unten.
    - **Nicht belegt:** Verhalten über mehrere Monitore (der Test hat einen Ausgang; nicht entschieden), fullscreen und
      maximized auf das fokussierte Fenster (nur nicht fokussiert), Abbruch der Auswahl von `area` mit Esc.
  - Lesart aus D2 (bei Widerspruch korrigieren):
    - **L38** `screenshot area` zählt als gewählt (`pal_chosen`, Log `done`), sobald niri die Aktion annimmt, also beim
      Öffnen der Auswahl, auch wenn der User sie danach mit Esc abbricht (niri meldet das Ende der Auswahl nicht).
  - Geprüft: `scripts/check.sh --only wayland` grün auf dem Endstand (**524 ok**, 492 s, Last 10–11,
    `logs/check/20261122-033753`); davor auf `395dfcc` 523 ok (558 s, Last 7–19, `logs/check/20261122-031652`). Der
    erste ganze Lauf hatte 520 ok und 2 rot: P23 maß die Ausgangshöhe der Spalte, bevor niri sie nach dem Zusammenführen
    neu gelegt hatte (875 statt 432); jetzt wartet der Test auf den Zustand (`col_shared`/`col_full`), danach zwei
    Wrapper-Läufe grün. Vorher Wrapper-Läufe je Abschnitt; **Wrapper:** Kopf von `run.sh` jetzt bis vor „# --- 1. Halten“
    per `awk` statt fester Zeilen (die Fakes machten den Kopf länger), `output.sh` ab Zeile 7 (`CLIP`).
    `fmt`, `clippy`, `cargo test --bin dictate-ui` nicht nötig (kein Rust geändert).
  - Gemessen: D2 rund 20 min bis zum Lauf von `wayland` (Aufräumen und Lesen bis ~02:50, Tests und Wrapper-Läufe bis 03:05, zwei
    Läufe von `wayland` je rund 9 min bis 03:26), Token nicht gemessen.
  - Nach der Durchsicht von D2: Fenster der Tests per PID aus `nested_spawn` beenden (nicht `pgrep -f`), Log-Prüfung um den
    Fehlertext des Fakes `systemctl suspend` und die Ausgabe des Fakes `pactl`, Überschrift in `run.sh` „P1.–P27.“.
  - **Frage F1 an Daniel** (entschieden 22.11., Entscheidung 17 in Abschnitt 7: Panel durchsichtig, dann schließen; Teil D3):
    Soll der Ausführer vor `screenshot full` und `area` warten, bis
    niri das Panel nicht mehr meldet, plus eine kurze Pause für die Schließ-Animation (Empfehlung: ja, Frist 500 ms, Pause
    etwa 200 ms; kostet die Verzögerung, macht das Ergebnis unabhängig vom Zufall), oder bleibt es beim gemessenen Stand
    (über die Palette 0 von 8 mit Panel)? Ein Ja ist Rust in `pal.rs`/`pal_host.rs` (Host wartet auf `own_window` = keins)
    mit Unit-Test und einer Wiederholung von P27 mit Animationen.
  - L31 entschieden: ohne Linie (wie die anderen Panels); L38 bestätigt (Entscheidung 17). Nächster Schritt: **Teil D3**
    (F1 wie entschieden, Auftrag in Entscheidung 17) in frischer Sitzung, dann **Block E** in frischer Sitzung (volle Suite samt `unit` ganz, das seit Block A nicht lief; unabhängige
    Durchsicht Sicherheit; Doku: die Tabelle der Prüfschritte in `CLAUDE.md` nennt `palette.sh` und `logout.sh`, README,
    ROADMAP, Vermerk in `PLAN-palette.md`; `docs/w5-vorlagen/` löschen; danach Fragen nach Installation, Merge und Push).
    Wrapper-Rezept: Kopf von `run.sh` per `awk` bis vor „# --- 1. Halten“, `output.sh` 7–98, `actions.sh` 7–134,
    `diagnose.sh` 12–58, Setup von `palette.sh` bis vor „# --- P1.“, dann die Abschnitte (P16b braucht P11).
- 22.11.: **Block D, Teil D3 fertig** (F1 nach Entscheidung 17 samt Nachtrag; Commits `2b8a48c` Zwischenstand, danach
  Fensterregel, P27 und Plan).
  - **Ablauf:** im Automaten `ui::cmdpal` ein Zustand „Abgang“ (`Leave`). `screenshot` `full`/`area`
    (`Intent::vanishes_first`) erzeugt statt `Hide` + `Execute` nur `Effect::Vanish { gen }`. Die Hülle
    (`PanelUi::vanish` in `pal_view.rs`) setzt die Deckkraft 1/255 und den Titel „dictate: leaving“, wartet über die
    Frame-Uhr auf zwei `after-paint` (das erste gehört zum durchsichtigen Frame, das zweite kommt erst nach dem
    Frame-Rückruf von niri, weil GDK die Uhr bis dahin anhält) und meldet `UiEv::Vanished(gen)`; nach `VANISH_MAX`
    (300 ms) führt der Automat trotzdem aus (Log „panel not repainted in time“). `Execute` trägt eine Nummer `run`:
    `PalMsg::Accepted { run }` (aus dem Arbeiter gleich nach `Handled`, vor `pal_chosen`) oder `Executed { run }` schließen
    das Panel, sonst `RUN_MAX` (2 s; die niri-IPC hat 1,5 s je Schritt). Ein spätes Ergebnis einer früheren Ausführung
    schließt nichts. `pal_chosen` bleibt nach `Handled` (L7/L38). `show` stellt Deckkraft und Titel vor `present` zurück
    (nie `hide`: ein deckender Frame vor dem Verbergen wäre wieder im Bild).
  - **Abweichung von Entscheidung 17:** Deckkraft 1/255 statt 0. Bei genau 0 baut GTK 4.24 keinen Knoten und übergibt
    keinen neuen Frame; niri behielt den deckenden (gemessen: Panel im Bild, Helligkeit 28, 3 von 3).
  - **Im Abgang** wirkt keine Eingabe (Tasten samt Esc, Klicks, Hover, Tippen, Antworten, Stimme). Sperre, Fokusverlust,
    Kürzel und Mod+Q vor dem Ausführen schließen: nichts ausgeführt, nichts im Verlauf, kein `pal_chosen`. Die Palette
    nimmt im Abgang keine Aufnahme an (`pal_takes_voice` für `claim` und Route); Mod+E dann wird ein Fensterdiktat.
    - **L39** (bestätigt als Entscheidung 19) Esc im Abgang wirkt nicht, auch nicht vor dem Ausführen: die Wahl ist mit Return getroffen, das Panel ist
      schon unsichtbar (abbrechen nur über Sperre, Fokusverlust, Kürzel). Bei Widerspruch korrigieren.
  - **Gemessen** (verschachteltes niri, Wrapper mit P22, P27 und einem Messteil; Fokusring 2 px, für die Messung Daniels
    Schatten per include; Vergleich gegen ein Bild ohne Panel im Panel und 40 Pixel darum):
    - Nur GTK-Deckkraft, ohne Regel: niri füllt die Fläche in Ringfarbe (`prefer-no-csd`, Helligkeit 167 statt 245),
      Ring und Schatten im Bild (Band um das Panel 60 682 von 60 760 Pixeln anders, darunter 32 554 von 64 350).
    - Mit der Regel: 0 Pixel Unterschied (3 von 3 ohne Animationen, mit Daniels Schatten und ohne).
    - Mit `window-close` 3000 ms linear (eigene Shader kann das verschachtelte niri mit pixman nicht): Esc und sofort
      `screenshot-screen` 8 von 8 **mit** Panel (Helligkeit 28–53), über die Palette 0 von 8 (0 Pixel Unterschied).
    - Frame-Uhr: der durchsichtige Frame ist nach 12–33 ms gemalt; den Frame-Rückruf schickt das verschachtelte niri
      erst nach 330–430 ms, mit Animationen nach 670–865 ms. Dort führt also meist die Frist aus; das Bild ist durch die
      Regel trotzdem sauber (niri wendet sie beim Titelwechsel an). Auf dem echten Desktop nicht gemessen.
    - `area`: niris Auswahl ohne Panel (`palette-shot-area.png`); Abbruch der Auswahl mit Esc: nichts in CLIPBOARD,
      danach kommen Tasten wieder an (bisher „nicht belegt“, jetzt in P27).
  - **Tests:** sieben neue Unit-Tests in `cmdpal_tests.rs` (Ablauf, `window` schließt zuerst, taub im Abgang, Schließen
    vor dem Frame, Fristen, frühere Nummer, Maus und Wahl nach der Antwort). P27: Reihenfolge „turning transparent“ <
    „run screenshot“ < „panel hidden“ (vor D3 rot: „panel hidden“ kam zuerst), Bild gleich dem ohne Panel (höchstens 200
    Pixel; ohne die Regel rot), `window` nie durchsichtig, `area` mit Esc, nach `full` und nach `area` beim nächsten
    Öffnen wieder deckend (`opaque_again`, dunkel im Bild; ohne das Zurückstellen in `show` rot); `shots_home` mit eigener
    Marke statt der Config (die Zeit der Config ändert sich bei einem Neuladen). Wrapper P22+P27 danach 41 ok.
  - **Geprüft:** `cargo test --release --lib ui::cmdpal` 31 ok, `--lib palette::ui_mirror` 5 ok, `--bin dictate-ui` 359 ok,
    `fmt`, `clippy`; `scripts/check.sh --only wayland,gui` grün (**wayland 531 ok** in 502 s, gui ok in 147 s,
    `logs/check/20261122-084241`). Gemessen: D3 rund 1 h 30 min (Lesen und Bau bis ~08:20, sechs Wrapper-Läufe mit Messung
    bis 08:40, Frage an Daniel, Prüflauf 11 min), Token nicht gemessen.
  - Daniels echte Config gelesen (nicht geändert): keine Fensterregel, die Deckkraft, Fokusring, Rahmen oder Schatten für
    alle oder schwebende Fenster setzt (nur `niri-hilfe` und unfokussiertes Alacritty); die Messung mit nachgebautem
    Ring und Schatten sollte also übertragbar sein. Nicht belegt: die Sperre im Abgang nur im Unit-Test (über `close`),
    nicht in `lock.sh`; der Weg „Frame gemeldet“ vor der Frist nur im Unit-Test (im verschachtelten niri läuft die Frist
    fast immer ab). Steht live immer „not repainted in time“ im Log, kostet das Warten 300 ms je Aufnahme: dann überdenken.
  - **Achtung Installation:** die Regel wirkt erst mit der KDL über chezmoi. Ein neues `dictate-ui` ohne die neue KDL
    macht Screenshots aus der Palette schlechter (Fläche in Ringfarbe); beide nur zusammen installieren.
  - Offen für den Live-Test: Dauer bis zum Frame auf dem echten Desktop (Log „panel: transparent after N ms“, „panel
    not repainted in time“), Bild mit Daniels Shadern.
  - Nächster Schritt: **Block E** (wie unter „Block D fertig“ beschrieben), zusätzlich: Durchsicht schließt `2b8a48c` und
    `4e70e52` ein (das unsichtbare Fenster hält bis ~2,3 s den Tastaturfokus; die Regel hängt am Titel); CLAUDE.md um die
    D3-Regeln (Deckkraft und Titel nur in `show` zurück, nie in `hide`; Titel „dictate: leaving“ und KDL-Regel gehören
    zusammen; `dictate-ui` und KDL nur zusammen installieren); aus „nicht belegt“ entfällt der Abbruch von `area` mit Esc;
    L39 und die Deckkraft 1/255 Daniel im Abschlussbericht vorlegen.
- 22.11.: **Block E fertig, W5 abgeschlossen** (Commits `2a3b2c6` Nachzug der Durchsicht, `40f60d9` Doku, danach
  dieser Eintrag). Daniel hat am 22.11. Installation, Merge und Push freigegeben: alle Binaries vom Stand `f902ea2`
  installiert, `dictate.service` und `dictate-ui.service` neu gestartet (10:19), `niri/dictate-binds.kdl` über chezmoi
  (dort lokal committet, nicht gepusht; `niri validate` der ganzen Config gültig), nach `main` gemergt, `main` und
  `wayland-w5` gepusht. Offen: Live-Test (Mod+Alt+Space, Screenshots aus der Palette mit Log „panel: transparent after
  N ms“, Textaktion mit Vorschau, Bestätigung per Klick).
  - **Volle Suite** zweimal grün: auf `c552237` (vor den Korrekturen, `unit` erstmals seit Block A) 7 ok in 1577 s
    (`logs/check/20261122-085909`, wayland 531 ok); auf dem Endstand `40f60d9` 7 ok in 1508 s (`logs/check/20261122-094159`:
    unit 576 s, **wayland 536 ok** in 481 s, gui 95 s, speech CER 0,0067 in 241 s, gpu 98 s). t18 wackelt wie bekannt: im ersten Lauf 1 von 3 über der Schwelle (CER 0,104, Mehrheit ok), im zweiten 2 von 2 bei 0,0; Schwelle nicht angefasst.
  - **Unabhängige Durchsicht Sicherheit** (Subagent Opus, nur Code, Bericht `/home/data/stt-test/logs/review/w5-security.md`):
    0 hoch, 1 mittel, 3 niedrig, 9 Hinweise. Ohne Befund: argv und Startweg, Ziele aus niri (feste ID, vor jeder Aktion
    neu geprüft), CLIPBOARD-Leser (ein Weg, Frist, Grenze, Hinweis `secret`), Sperre, Bestätigung per Tastatur, eine
    Antwort je Auftrag.
    - **M1** `text_action` aus der Palette sandte PRIMARY ohne Vorschau (Lua-Parität, widersprach „nie ohne Vorschau
      senden“): jetzt Vorschau, die vorgewählte Aktion erst auf Return (Entscheidung 18; `ui::panel`, Unit-Test, P17 prüft
      „nothing is sent before the confirmation“).
    - **N1** mit `DICTATE_DEBUG` standen Zwischenstände und final einer Aufnahme der Palette im Log (Daemon, Streamer,
      KI-Nachbearbeitung): `log_text` über `Work::Begin` (nur Fensterdiktat), Test `tests/daemon_voice_log.rs` (eigenes
      Binary, setzt `DICTATE_DEBUG`, schneidet das Log mit; Gegenprobe ohne Korrektur rot).
    - **N2** Fensterregel per Titel kann jedes Programm für sich nutzen: hingenommen (wer die Rechte des Users hat, kann
      die KDL selbst schreiben), Begründung im Kopf der Regel.
    - **N3** ein zweiter Klick auf die Zeile einer offenen Bestätigung führte aus (langsamer Doppelklick auf „Power
      off“): jetzt nur Return oder „Confirm“ (Entscheidung 18, Unit-Test).
    - Hinweise: **H1** Wächter `code()` schnitt `//` auch in Strings, Startwege `CommandExt`, `spawn_async`, `AppInfo`,
      `UriLauncher`, `show_uri`, `posix_spawn`, `libc::exec`, `"Spawn"`, `Command as` nicht erkannt: behoben (Test
      `comments_are_cut_only_outside_strings`). **H3** Richtungssteuerzeichen in Adressen jetzt auch in `ui::pal_url`
      abgelehnt (Liste wie im Katalog). **H9** Ladefehler von `palette.json` ohne den Wert (serde zitiert ihn). **H8**
      Verlauf überlebt die Sperre (Entscheidung 18: bleibt). Hingenommen: H4 `SetColumnDisplay` ohne ID (geht mit der
      IPC nicht anders), H5 Abgang nimmt nach 300 ms auch ohne Frame auf (Log meldet es), H6 unsichtbares Panel hält den
      Fokus bis ~2,3 s (Tasten wirken nicht, werden nicht geloggt, beim Verbergen geleert), H7 `PATH` der Sitzung.
      Offen: H2 eigenes `Debug` mit Schwärzung bzw. Test gegen `{:?}` auf Palettentypen (heute nicht genutzt).
    - Außerhalb des Umfangs gesehen (offen): `output.rs` loggt `app_id` bzw. Fenster-IDs der Ausgabe (älter als W5);
      die gesprochene Eingabe der Palette läuft durch die ganze Pipeline des Diktats samt KI-Modus (`pipeline::process`),
      wie unter Awesome; Daniel fragen, ob die Palette den Rohtext bekommen soll.
    - Unsicher (außerhalb des Repos): niri vergibt Fenster- und Arbeitsflächen-IDs nie doppelt; `xdg-open` mit der URL
      als einem argv-Element; `FocusLost` beim Sperrbildschirm.
  - **Doku:** CLAUDE.md (Stand, Offen 2/5/7, Kürzel, Prüftabelle mit `palette.sh`/`logout.sh`, Regeln aus W5 und D3,
    Dauer der Suite), README (Befehlspalette unter niri, `Mod+Alt+Space`, Installation: Binary und KDL zusammen), ROADMAP
    (Bausteine 3, 4, 6), Vermerk in `PLAN-palette.md`, Seite „Palette“ ohne „tags“ und ohne „later version“;
    `docs/w5-vorlagen/` gelöscht, Verweise zeigen auf `c552237:docs/w5-vorlagen/…` (`palette-smoke.sh` auf `f660676`).
  - **Nicht belegt** (für den Live-Test): mehrere Monitore; fullscreen/maximized auf das fokussierte Fenster; Sperre im
    Abgang (nur Unit-Test); Weg „Frame gemeldet“ vor der Frist (nur Unit-Test, im verschachtelten niri läuft die Frist
    fast immer ab). Belegt seit D3: Abbruch von `area` mit Esc.
  - Gemessen: Block E rund 1 h 20 min (Aufräumen und Start 08:50, erste Suite bis 09:25 parallel zur Durchsicht 18 min,
    Korrekturen bis 09:40, Abschluss-Suite bis 10:07), Token nicht gemessen.

## 6h. Auftrag W6: Einstellungsfenster in Noctalia-Farben (Entscheidung 9)

Stand vorher (22.11.): W1–W5 und W7 gemergt, gepusht und installiert (`main` `c315203`, Binaries vom Stand `f902ea2`);
Branch `wayland-w6` von `main` `c315203` im Worktree `wayland-w5` (Build-Cache warm). Live-Test von W2–W5 durch Daniel
offen; er testet erst nach allen Blöcken (Befunde daraus haben Vorrang, sobald er sie meldet). Grundlage dieses
Auftrags: Bestandsaufnahme `/home/data/stt-test/logs/w6-bestandsaufnahme.md` (Subagent, 22.11.; Befunde dort
**abgeleitet** aus dem Code, zwei davon stichprobenartig nachgeprüft: `dictate-settings` bindet nur `config`, `actions`
und `gui` per `#[path]` ein, nicht die Bibliothek; `tests/gui` prüft keine Pixel) und Referenzbilder aller 14 Seiten vor
W6: `/home/data/stt-test/logs/w6-ref/before-<seite>.png` und `before-<seite>-2.png` (28 Bilder, `tests/gui/shots.sh`
gegen `target/release/dictate-settings` vom 22.11. 09:51, also nach `40f60d9`, dem letzten Commit an `src/gui`; im
verschachtelten niri gegen den Mock, sauber abgebaut). Kennzeichnung wie oben: **gemessen**, **abgeleitet**, **offen**.

### Ziel

`dictate-settings` sieht aus wie der Rest von dictate unter niri: Farben aus den Material-Rollen der Noctalia-Palette
(dieselbe Datei wie `dictate-ui`, Rückfall Tokyo Night dunkel), live umgefärbt, wenn Noctalia das Schema wechselt;
Schrift aus den GTK-Einstellungen; Ecken wie Noctalias Panels (12 Fenster, 8 Felder/Knöpfe/Zeilen, 4 Chips); dazu
richtige Texte für niri, eine Vorschau des Overlays wie das echte Overlay, Logo und Marke nach Vorlage V3/V4, Doku
(`DESIGN.md`, README, CLAUDE.md, ROADMAP) nach V5. Bedienkonzept und Seitenaufbau bleiben (Seitenleiste 240, Inhalt
höchstens 720, Gruppen durch Label und Abstand, Chips statt Schalter, Apply/Revert), ebenso die Fenstergröße
1180x860 (`gui_start` prüft sie).

### Befunde aus der Bestandsaufnahme (Kurzfassung)

- **Dateikollision `palette.json` (abgeleitet, nicht nachgestellt):** `~/.local/state/dictate/palette.json` ist zugleich die
  Farbdatei aus der Noctalia-Vorlage (`src/ui/palette.rs` 90–99, `~/.config/noctalia/config.toml` 64) und die
  Häufigkeitsdatei der Befehlspalette (`src/palette/sources/frecency.rs` 85, `src/daemon.rs` 2081). Ein Speichern der
  Häufigkeit ersetzt die Farbdatei per `rename`, `dictate-ui` fällt dann mit Warnung auf Tokyo Night zurück; ein
  Schemawechsel in Noctalia überschreibt die Häufigkeit, der Daemon beginnt mit Warnung leer (`frecency.rs` 234–270).
  Heute unsichtbar, weil das Schema auf blub Tokyo Night ist und die Palette unter niri erst seit W5 (heute
  installiert) wieder Wahlen speichert. Die Farbdatei auf blub ist vom 20.11. 13:59 (alle 28 Rollen); eine frühere
  Häufigkeit aus der Zeit unter Awesome ist damit vermutlich schon überschrieben. Vorlage V1.
- **`src/ui/palette.rs` braucht kein GTK** (nur `serde_json`, `std`, `ui::fsutil` mit `libc`), liest neben `mode` (Feld `dark`) nur 6 der 28
  Rollen (`primary`, `error`, `surface`, `on_surface`, `on_surface_variant`, `outline_variant`); das Beobachten
  der Datei liegt in `dictate-ui` (`app.rs` 634–678, gio). `size::RADIUS` hängt an `ui::view` (und damit an `protocol`).
- **Thema heute:** 13 feste Farben in `src/gui/theme.rs` (Pushing Pixels), 79 Verwendungen in 12 Dateien, keine
  Farbliterale außerhalb; `theme::install` setzt rund 45 Stilwerte, `dark_mode = true` fest; alle Ecken 0; Schrift
  Google Sans Flex über `fc-match` (nicht eingebettet), Basis 14 px; Labels und Seitentitel in Versalien mit Laufweite.
- **Veraltete Texte:** `pages/palette.rs` 276 („no panel and no key yet“, falsch seit W5), `pages/general.rs` 68–80
  (Kürzel ohne Mod+Shift+I und Mod+Alt+Space). „X11 class under XWayland“ (`actions.rs` 155, `profiles.rs` 51) ist
  richtig (xwayland-satellite) und bleibt (L44).
- **Overlay-Vorschau** (`pages/overlay.rs` 10–30) zeigt noch das alte Overlay (eckig, 5x2 Zellen, feste Farben); das
  echte Overlay ist seit W1 `surface` mit Radius 12 und `outline_variant` (`src/ui/view.rs`, `overlay.rs` 192–206).
- **Logo und Marke:** Fenster-Icon `assets/logo/icon-512.png` (`dictate-settings.rs` 46), About mit
  `src/gui/assets/lockup-on-dark.png` und `pushingpixels-glyph-on-dark.png` (beide hell auf transparent, auf hellem Grund
  unlesbar; Quelle der PNGs nicht dokumentiert, kein Skript). `assets/brand/` wird im Code nicht verwendet,
  `design/pushingpixels/project/` (13 Dateien) ist die Kopie des Guides, auf die `DESIGN.md` verweist.
- **GUI-Tests:** keine Pixel- oder Farbprüfung, ein Farbwechsel bricht sie nicht; aber 14 Klickstellen mit 9 festen
  Koordinatenpaaren und 7 Scroll-Stellen in `smoke.sh`, `diagnose.sh`, `interact.sh` hängen an Schrift, Abständen und
  Umbrüchen. Der Harness isoliert `XDG_*` und setzt `GSETTINGS_BACKEND=memory`, nicht `HOME`; `tests/gui/lib.sh` setzt
  kein `DICTATE_PALETTE`. Zwei Bibliothekstests lesen GUI-Quelltext: `src/palette/intent.rs` 641
  (`Page::label` in `src/gui/mod.rs`) und `src/pipeline/commands.rs` 515 (`pages/commands.rs`).

### Regeln (gelten für jeden Block)

1. **Nur die 28 Rollen der installierten Vorlage** (`noctalia/dictate.json.tmpl`, gleiche Liste wie niri-coach). Eine
   neue Rolle hieße: Vorlage ändern und über chezmoi neu installieren, also eigene Vorlage an Daniel. Eine Rolle
   `outline` gibt es nicht (nur `outline_variant`).
2. **Eine Quelle für Pfad, Parser und Rückfall:** `src/ui/palette.rs` wird erweitert und in `dictate-settings` per
   `#[path]` eingebunden (mit `ui/fsutil.rs` als Geschwister), keine zweite Kopie; `RADIUS` zieht nach `palette.rs`
   (oder ein kleines gemeinsames Modul ohne `protocol`). Wer `src/ui/palette.rs` ändert, prüft auch `dictate-ui`
   (Schritt `wayland`, `cargo test --release --bin dictate-ui`, `--lib ui::palette`).
3. **Hell/Dunkel nur aus `mode` der Palette** (nach V2), nie aus einer zweiten Quelle (kein Portal, kein gsettings).
4. **Die Datei ist fremde Eingabe:** nur gewöhnliche Datei bis 64 KiB (`MAX_BYTES`, `fsutil::read_regular`), Fehler =
   Rückfall mit Grund im Log, nie Absturz; das Beobachten verfolgt Ersetzen per `rename` (Noctalia schreibt so) und
   Löschen; Entprellung wie `dictate-ui` (50 ms). Kein Dauer-Neuzeichnen im Leerlauf (Messpunkt M3).
5. **Kontrast:** jede Textpaarung mindestens 4,5:1 bei Tokyo Night dunkel (Unit-Test über die Zuordnung Rolle → Stil);
   helle Schemata werden nur gemessen (Ausgabe des Tests, M5), die Farben bleiben exakt wie Noctalia sie liefert
   (Entscheidung 21, geändert am 22.11.; vorher „auch bei einer hellen Testpalette“). Text in `on_*`-Rollen auf ihrer
   Fläche, Nebentext `on_surface_variant`; dazu `primary` und `error` als Text auf `surface` (L49, Entscheidung 21).
6. **Tests nie gegen Daniels Einstellungen:** Palette und GTK-Schrift im Test aus Fixtures im Laufzeitordner des
   verschachtelten niri (`DICTATE_PALETTE` bzw. `XDG_CONFIG_HOME` des Harness), nie `~/.config` oder `~/.local/state`
   direkt; Config `~/.config/dictate/config.toml` nie anfassen; nie `niri msg` an das echte niri.
7. **KDL und Binary nur zusammen:** eine Fensterregel für `dictate-settings` (Ecken) kommt nur zusammen mit dem
   passenden Binary auf blub, über chezmoi und nur mit Daniels Zustimmung; `tests/gui/lib.sh` bindet die KDL ein
   (Schritt `gui` prüft das Schweben).
8. **Sichtbare Texte englisch** und nach den Textregeln (knapp, keine Gedankenstriche, keine Emoji); Doku, Kommentare,
   Commits deutsch (Entscheidung 1).

### Vorlagen an Daniel (je einzeln, mit Empfehlung)

Entschieden am 22.11.: alle wie empfohlen (Entscheidung 20).

- **V1 Dateikollision `palette.json`.** (a) Welche Datei zieht um: **Empfehlung: die Häufigkeit der Befehlspalette**
  nach `~/.local/state/dictate/frecency.json`. Die Farbdatei folgt derselben Regel wie doorcam und niri-coach
  (`<tool>/palette.json` in der Noctalia-Config), sie umzubenennen hieße Vorlage, Noctalia-Config und chezmoi ändern;
  die Häufigkeit ist ein internes Detail des Daemons. Übernahme beim Start nur nach Inhalt: trägt `palette.json`
  `version` und `entries` und keine Farbrollen und gibt es `frecency.json` noch nicht, wird sie dorthin umbenannt; eine
  Farbdatei wird nie gelöscht oder verschoben. (b) Wann: **Empfehlung: als Block A von W6**, installiert mit W6
  (Daniel testet ohnehin am Ende; bis dahin ist der Schaden höchstens eine verlorene Häufigkeit bzw. ein Rückfall auf
  dasselbe Tokyo Night). Alternative: sofort als eigener kleiner Branch mit Installation.
- **V2 Hell/Dunkel.** **Empfehlung: dem `mode` der Palette folgen, live** (egui `set_theme` bzw. Stil je Thema). Wer in
  Noctalia ein helles Schema wählt, bekommt dann auch ein helles Einstellungsfenster wie das Overlay. Alternative:
  immer dunkel (nur die Akzentfarbe folgt).
- **V3 Logo von dictate.** **Empfehlung: das Pixelzeichen bleibt, wird aber in egui aus Rollen gezeichnet** (Zellen
  `on_surface` mit Deckkraft, Akzentzelle `primary`, wie die Pixelwelle im Overlay) statt als festes PNG; damit passt es
  zu hell und dunkel und zu jedem Schema. Wortmarke „dictate“ als Text in der Titelschrift. Das Programm-Icon
  (`assets/logo/icon.svg`, `icon-512.png`, im Starter und Fenstertitel) bleibt unverändert, weil Icons nicht
  umgefärbt werden. Alternativen: PNGs wie heute behalten (unlesbar bei hellem Schema) oder kein Logo, nur Wortmarke.
- **V4 Marke Pushing Pixels auf der Seite About.** **Empfehlung: Text und Link bleiben („Daniel Kurdoghlian“,
  „Pushing Pixels, Vienna“, `pushingpixels.at`), das Zeichen (Bild) entfällt.** Die Regeln der Marke verbieten
  Nachzeichnen und Umfärben, die Datei ist nur hell auf dunkel und wäre bei hellem Schema unsichtbar. Alternativen:
  Zeichen nur im dunklen Modus zeigen, oder Zeichen immer behalten (dann bei hellem Schema auf eigener dunkler Fläche).
- **V5 `DESIGN.md`, `assets/brand/`, `design/pushingpixels/`.** **Empfehlung: `DESIGN.md` neu fassen** für die ganze
  Anwendung unter Noctalia (Rollen und ihre Verwendung in `dictate-ui` und im Einstellungsfenster, Ecken, Schrift aus
  GTK, 8-px-Raster, Chips, Gruppen, Kontrastregel, Textregeln, Logo nach V3); was nicht mehr gilt (Pushing-Pixels-Tokens,
  Versalien, Ecken 0, Overlay-Archiv), entfällt und bleibt in der Git-Geschichte. `design/pushingpixels/` und (nach V4)
  `assets/brand/` werden gelöscht, die PNGs unter `src/gui/assets/` ebenso, soweit V3/V4 sie nicht mehr brauchen.
  Alternative: alte `DESIGN.md` als Archiv mit Vermerk behalten und eine neue daneben.

### Lesarten von Claude (bei Widerspruch korrigieren)

Nummern setzen die Lesarten aus 6g fort.

- **L40** Typografie wie das Panel in `dictate-ui`: keine Versalien und keine Laufweite für Labels und Seitentitel
  (Gewicht 600 für Titel, Label in `on_surface_variant`), Größen als Verhältnis zur GTK-Basisschrift (das Panel nutzt
  0,8/0,85/1,1 em). Das Versalien-Muster gehört zum Pushing-Pixels-Design, das mit Entscheidung 9 entfällt.
- **L41** Schrift: Familie und Größe aus `gtk-font-name` (Quelle nach Messpunkt M1), aufgelöst über `fc-match` wie
  heute `find_font`; Mono aus `monospace-font-name`, sonst `monospace`. Fehlt alles, gilt egui-Vorgabe. Google Sans Flex
  entfällt als feste Wahl. **Geändert nach M1 (Block B, Lesart von Claude, bei Widerspruch korrigieren):** Quelle ist
  nur `settings.ini` wie bei GTK 4 (`$XDG_CONFIG_HOME/gtk-4.0/settings.ini`, sonst `~/.config/gtk-4.0/settings.ini`;
  ob GTK zusätzlich `XDG_CONFIG_DIRS`/`/etc/gtk-4.0` liest, ist nicht gemessen, auf blub gibt es dort nichts), kein
  gsettings, kein Portal. `monospace-font-name` ist ein gsettings-Schlüssel und damit keine Quelle; Mono ist
  `monospace` über `fc-match` (wie `font-family: monospace` im Panel von `dictate-ui`). Fehlt die Datei oder der
  Schlüssel, gilt die Vorgabe von GTK `Adwaita Sans 11` (gemessen), nicht die von egui, damit beide Fenster gleich
  bleiben. **Ergänzt in Teil 2 (gemessen, Nachmessung unter M1; Entscheidung 21):** `gtk-font-name` wird wie eine
  Pango-Beschreibung zerlegt (letztes Wort = Größe, `pt` oder mit `px`; Stil- und Gewichtswörter davor fallen weg, GTK
  übernimmt sie nicht), nur die Familie geht an `fc-match`, nie die ganze Zeichenkette; Größe in logischen Pixeln
  = pt × dpi / 72 mit dpi aus `gtk-xft-dpi` / 1024 (Vorgabe 96). Ohne Prüfung auf den Dateinamen: was fontconfig als
  Ersatz liefert, zeigt GTK auch.
- **L42** Ecken: Felder, Knöpfe, Zeilen der Seitenleiste 8, Chips und Kästchen 4, egui-eigene Popups 12; das Fenster
  selbst rundet niri über eine Fensterregel `geometry-corner-radius 12` mit `clip-to-geometry true` für `app_id`
  `dictate-settings` in `niri/dictate-binds.kdl` (wie beim Panel), Regel 7.
- **L43** Beobachten in egui ohne neue Abhängigkeit: inotify über `libc` auf den **Ordner** der Datei (fängt `rename`
  und Neuanlage), eigener Thread, Entprellung 50 ms, dann `ctx.request_repaint()` (Muster `src/gui/net.rs`); das Thema
  wird im nächsten Frame neu gesetzt. Fällt inotify aus, Rückfall auf Prüfen von `mtime` alle 2 s mit
  `request_repaint_after` (Grund im Log).
- **L44** „X11 class under XWayland“ bleibt in den Texten (`actions.rs` 155, `profiles.rs` 51): technisch richtig,
  Programme unter xwayland-satellite melden ihre Klasse als `app_id`.
- **L45** Die Overlay-Vorschau zeichnet mit denselben Maßen und Rollen wie `dictate-ui` (Radius 12, `surface`,
  `outline_variant`, Pixelwelle 7x5), verkleinert; Konstanten aus einer Quelle (Regel 2), nicht abgeschrieben.
- **L46** Klickziele der GUI-Tests aus einem Lage-Log des Fensters (Muster `panel: layout <id> …` in `dictate-ui`,
  nur mit `DICTATE_DEBUG` bzw. einem Testschalter), nicht mehr als feste Zahlen; so verschiebt eine andere Schrift die
  Tests nicht mehr. Feste Zahlen nur noch für das Scrollen bis zum Ziel, falls nötig.
- **L47** Die Kürzelgruppe auf der Seite General nennt alle sieben Kürzel aus `niri/dictate-binds.kdl`; ein Test hält
  Seite und KDL gleich (wie `gui_fallback_table_in_sync`).
- **L48** (Block A) Die Übernahme der alten Häufigkeit (`frecency::adopt_legacy`) läuft bei jedem Start des Daemons,
  auch wenn Palette oder Häufigkeit aus sind: sie rettet nur Daten des Daemons vor dem nächsten Schreiben von Noctalia
  und lädt nichts in den Speicher der Häufigkeit („Aus = nichts lesen“ gilt für das Zählen, nicht für das Umziehen).
  „Umbenennen“ ist als Kopie der einmal gelesenen Bytes ohne Ersetzen (`hard_link`) plus Entfernen der alten Datei nur
  bei gleichem Inode umgesetzt, damit ein gleichzeitiges `rename` von Noctalia nie die Farbdatei verschiebt; erkannt
  wird streng (genau `version` als Ganzzahl und `entries` als Objekt, kein weiterer Schlüssel). **Berichtigt in Block E
  durch L54.**
- **L49** (Block C) Regel 5 sagt „Text nur in `on_*`-Rollen auf ihrer Fläche“; das Fenster zeichnet wie das Panel von
  `dictate-ui` trotzdem `primary` (Links, gewählter Chip, Akzent-Labels, Kürzel) und `error` (Meldungen unter Feldern)
  als Text auf `surface`, weil Material 3 Akzent- und Fehlertext so vorsieht und das Panel es seit W3 so tut. Die
  Kontrastprüfung schließt diese Paare ein. Von Daniel bestätigt (Entscheidung 21).
- **L50** (Block C Teil 2) Zuordnung der Größen nach L40 (nennt nur 0,8/0,85/1,1 em), `theme::Size`: 1 em Fließtext,
  Feldlabels (Nebentext), Seitenleiste, Knöpfe und Chips (Gewicht 400 wie `.dp-btn`; erst 600 versucht, wirkte neben
  den Chips zu schwer); 0,85 em Hinweise, Meldungen unter Feldern, Textknöpfe in Tabellen und Mono (wie `.dp-meta`,
  `.dp-report`, `.dp-chip`); 0,8 em Gruppenlabels (Gewicht 600, `on_surface_variant`); 1,1 em Seitentitel (Gewicht 600,
  `on_surface`). Keine Laufweite, auch nicht bei den Gruppenlabels, obwohl `.dp-group` im Panel `letter-spacing: 1px`
  hat (L40 wörtlich; Abweichung zum Panel hier vermerkt). Die Wortmarke „DICTATE“ in der Seitenleiste bleibt bis zum
  Logo nach V3 (Block D) in Versalien. Gewicht 600: dieselbe Datei mit Achse `wght`, sonst die statische Datei, die
  fontconfig für `semibold` nennt (auf blub bei Roboto `Roboto-Bold.ttf`, 700, weil es keine SemiBold gibt).
- **L51** (Block C Teil 2) Rückfall ohne inotify (L43): `mtime`, Größe und Inode alle 2 s **im Thread des Beobachters**
  geprüft, geweckt wird nur bei einer Änderung; `request_repaint_after` im Fenster (Wortlaut L43) zeichnete im
  Leerlauf alle 2 s neu und bräche M3. Fehlt der Ordner, beobachtet der Thread den nächsten vorhandenen Vorfahren.
- **L52** (Block D) Overlay-Vorschau (L45): Bildschirm und Overlay im **selben** Maßstab, damit Lage, Abstand und
  Breite stimmen; der Bildschirm ist wie bisher als 1920x1080 angenommen (das Overlay selbst misst den echten), die
  Vorschau ist so breit wie die Inhaltsspalte (höchstens 720, Maßstab dann 0,375; vorher 320 breit, Zellen der Welle
  wären unter 1 px). Gezeigt wird das Overlay bei langer Mitschrift (größte Breite, zwei Zeilen); die Schrift wäre bei
  diesem Maßstab unleserlich, darum Text als Balken (`on_surface`, Tail `on_surface_variant`). Die Welle ruht mit
  festen mittleren Pegeln (kein Zeitgeber, M3). Ein Abstand, der das Overlay aus dem Bild schöbe, hält es in der
  Vorschau am Rand. Maße, Welle, Deckkraft der Töne und Zellradius aus `src/ui/wave.rs` (neu, ohne `protocol`, aus
  `view.rs` herausgelöst; `dictate-settings` bindet es per `#[path]` ein, `view.rs` reicht es weiter). Abwägung:
  die Vorschau ist bei 720 Breite 405 hoch (vorher 180), die Felder der Gruppe „Placement“ rutschen dadurch unter den
  ersten Ausschnitt; wer Abstand, Breite oder Deckkraft ändert, sieht die Vorschau nur nach Scrollen ganz. Kleiner
  ginge nur auf Kosten der Welle (bei 480 Breite Zellen von rund 1,5 px). Bei Daniels Live-Test beurteilen.
- **L53** (Block D) Glühen entfällt: weder Noctalia noch das Panel von `dictate-ui` kennen es. Hauptknopf wie ein
  gefüllter Knopf von Material 3, Hover als Zustandsschicht `on_primary` 8 % (`theme::STATE_LAYER`, als Paar im
  Kontrasttest); dasselbe Kriterium für den glühenden Punkt „Daemon running“ in der Seitenleiste und den Punkt auf der
  Seite AI (`widgets::pixel` ohne `glow`). Logo nach V3 in den Verhältnissen von `assets/logo/lockup-on-dark.svg`
  (Schriftgröße, Abstand, Grundlinie und Laufweite relativ zur Höhe des Zeichens, `widgets::lockup`; Test
  `logo_follows_the_svg` hält Zellen und Maße gleich dem SVG), Seitenleiste 16 px hoch, About 36 px.

- **L54** (Block E, berichtigt L48; Befund der unabhängigen Durchsicht) Die alte Datei wird nach der Übernahme bzw. beim
  „Clear“ nicht mehr per Namen entfernt, sondern über ihren eigenen Deskriptor geleert (`Legacy::empty`: Wiederöffnen
  über `/proc/self/fd`, erst nach der Erkennung, dieselbe Inode, genau ein Name, Länge 0, `fsync`; bei der Übernahme
  vorher `frecency.json` und der Ordner dauerhaft). Grund: Linux kennt kein „Entfernen nur bei dieser Inode“; zwischen
  der Prüfung und dem `unlink` hätte ein `rename` von Noctalia die Farbdatei unter den Namen gelegt, und sie wäre
  gelöscht worden (V1 verlangt „nie“). Das Leeren trifft dann nur die schon verdrängte alte Inode. Folge:
  `palette.json` bleibt als leere Datei stehen; für Overlay, Panels und Fenster gilt wie bei einer fehlenden Datei
  Tokyo Night, bis Noctalia neu schreibt; der nächste Start erkennt sie nicht als Häufigkeit. Grenze: gilt, solange
  andere Schreiber per `rename` ersetzen (Noctalia tut es). Test `a_color_file_renamed_in_after_the_check_survives`
  (vorher rot). Verschärft nur in Richtung von V1, daher ohne Rückfrage (bei Widerspruch korrigieren).

### Messpunkte

- **M1 (erledigt, Block B, 22.11.)** Welche Quelle nimmt GTK 4 in `dictate-ui` auf blub für die Schrift (Portal `org.freedesktop.portal.Settings`,
  gsettings, `settings.ini`)? Im verschachtelten niri mit `GSETTINGS_BACKEND=memory` und gesetzter `settings.ini`
  nachprüfen, live nur nach Daniels Test. Das Einstellungsfenster folgt derselben Quelle (L41).
  **Gemessen** mit einer GTK-4-Probe (PyGObject, dieselbe libgtk 4.24.1 wie `dictate-ui`, laut `ldd` beide `/usr/lib/libgtk-4.so.1`) im Harness, 14 Fälle in vier
  Läufen, Skripte, Portal-Attrappe und Tabelle unter `/home/data/stt-test/logs/w6-m1/` (`ergebnis.md`): außerhalb einer
  Sandbox und ohne `GDK_DEBUG=portals` liest GTK die Schrift **nur aus `settings.ini`**; gsettings (auch ein keyfile mit
  gesetztem `font-name`, das `gsettings get` liest) und das Settings-Portal (Attrappe auf privatem Session-Bus, nie
  gefragt) wirken nicht, `GTK_USE_PORTAL` ist ohne Einfluss. Ohne Datei gilt `Adwaita Sans 11`. Nur mit
  `GDK_DEBUG=portals` fragt GTK das Portal (`ReadAll`), und dann gewinnt es vor `settings.ini`. **Abgeleitet** für den
  Prozess `dictate-ui` auf blub (nicht im Prozess gemessen): keine `GDK_*`/`GTK_*`-Variable, keine Sandbox, also
  `~/.config/gtk-4.0/settings.ini` mit `gtk-font-name=Roboto 12`; dconf sagt ebenfalls `Roboto 12`. ~~Befund für
  Daniel: Roboto ist auf blub nicht installiert, gerendert wird Noto Sans.~~ **Berichtigt (22.11., Entscheidung 21):**
  Messfehler. Roboto ist seit 18.03.2024 installiert (`ttf-roboto` 3.016-1); gefragt war `fc-match 'Roboto 12'`, das
  fontconfig als Familie „Roboto 12“ liest (Rückfall Noto Sans). GTK zerlegt die Pango-Beschreibung und zeigt Roboto.
  Frage an Daniel damit erledigt. Folge: L41 geändert (siehe dort), Schrift-Fixture der GUI-Tests ist `settings.ini`
  (seit Block C Teil 2 mit `Roboto 12`). Dauer gemessen rund 5 min (vier Läufe zu je unter 10 s, dazu Lesen und
  Ablegen).
  **Nachmessung Größe (Block C Teil 2, 22.11.):** dieselbe Probe im Harness (Ausgabe 1,625; Skripte und Ergebnis unter
  `/home/data/stt-test/logs/w6-c/m1b/`) mit Pango-Kontext und gerendertem Lauf eines Labels: `Noto Sans 12` → 16 px
  (Auflösung 96), mit `gtk-xft-dpi=122880` (120 dpi) → 20 px, `Noto Sans 16px` → 16 px, ohne Datei `Adwaita Sans 11`
  → 14,667 px. Also px = pt × dpi / 72 mit dpi = `gtk-xft-dpi` / 1024 (Vorgabe 96), Einheit `px` gilt direkt; das sind
  logische Pixel, wie egui sie zählt. `Noto Sans SemiBold 11` wird als `Noto Sans 11` gerendert: GTK 4 übernimmt aus
  `gtk-font-name` nur Familie und Größe, nicht das Gewicht.
- **M2** Zeit vom Schreiben der Palette bis zum umgefärbten Frame (Log „theme: palette applied after N ms“), Ziel unter
  200 ms.
- **M3** Leerlauf: Zahl der Frames in 10 s ohne Eingabe mit laufendem Beobachter (egui zeichnet nur auf Ereignis),
  vorher/nachher gleich.
- **M4** Startzeit bis zum ersten Frame vorher/nachher (zusätzliche `fc-match`-Aufrufe).
- **M5** Kontrastwerte der Textpaare bei Tokyo Night und der hellen Testpalette (Unit-Test gibt sie aus).

### Zuschnitt (je Block eine frische Sitzung)

| Block | Inhalt | Prüfung | gesperrt durch |
|---|---|---|---|
| **A** Kollision `palette.json` | zuerst ein Test, der scheitert (Daemon speichert eine Wahl, die Farbdatei bleibt unverändert); dann Häufigkeit in eigene Datei nach V1, Übernahme nur nach Inhalt (Tests: alte Häufigkeit wandert, Farbdatei bleibt, beide vorhanden = nichts verschieben), Doku-Stellen der Pfade (`frecency.rs`, `daemon.rs`, README) | `fmt,clippy,unit` | V1 |
| **B** Test-Infrastruktur | zuerst Messpunkt M1 (welche Quelle GTK 4 für die Schrift nimmt), danach die Schrift-Fixture nach dieser Quelle; Lage-Log des Fensters (L46), `smoke.sh`, `diagnose.sh`, `interact.sh` lesen Klickziele daraus; Fixtures unter `tests/gui/fixtures/` (Palette dunkel und hell, `gtk-4.0/settings.ini` mit fester Schrift) und `lib.sh` setzt sie; alles auf dem **unveränderten** Design grün | `fmt,clippy,gui`, `cargo test --release --bin dictate-settings` | – |
| **C** Thema zur Laufzeit | `Palette` um die nötigen Rollen erweitern (Regel 1/2, Test `parse_template_output` nachführen), `RADIUS` verlegen; `theme.rs` als Thema aus Rollen statt Konstanten (79 Stellen), Hell/Dunkel nach V2, Schrift nach M1/L41, Typografie L40, Ecken L42 innen, Beobachter L43; Kontrasttest (Regel 5); neuer GUI-Test: Palette im laufenden Fenster tauschen, ein Pixel der Seitenleiste hat danach die neue Farbe; Messpunkte M2–M5 | `fmt,clippy,unit,gui,wayland`, `--bin dictate-ui`, `--bin dictate-settings` | V2 |
| **D** Seiten, Logo, Texte | Overlay-Vorschau (L45), Logo nach V3, About nach V4, Texte (`palette.rs` 276, General-Kürzel mit Test L47), Doc-Kommentare in `src/gui` ohne Pushing-Pixels-Bezug, Fensterregel für die Ecken (L42) in der KDL; Screenshots aller Seiten dunkel und hell mit `shots.sh`, selbst bewerten, mit den Referenzbildern vergleichen; Daniel nur die nötigen Bilder zeigen | `fmt,clippy,gui,wayland` (KDL), bei `Page::label`/`pages/commands.rs` auch `--lib palette::intent` und `--lib pipeline::commands` | V3, V4 |
| **E** Abschluss | `DESIGN.md`, `assets/brand/`, `design/pushingpixels/`, PNGs nach V5; README (Design, Palette-Datei, Häufigkeit), CLAUDE.md (Stand, Fallen/Design, Prüftabelle), ROADMAP; volle Suite; unabhängige Code-Durchsicht (neue Architektur des Themas, über 300 Zeilen); Liste für Daniels Live-Test W2–W6; danach Fragen nach Installation (Binaries plus KDL über chezmoi), Merge und Push | volle Suite | V5 |

**Schätzung:** fünf Sitzungen (A–E), C eine bis zwei. Messen nach jedem Block (Dauer; Token sieht die Sitzung nicht).

### Stand W6

- 22.11.: **W6-0** (diese Sitzung, kein Produktcode): vorige Sitzung (Block E von W5) aufgeräumt (Zustand „done“; keine
  Worker im Repo dictate, die Worker des Data Room nicht angefasst; Chat-Tab kann die CLI nicht schließen), keine
  Testumgebung lief (`wl-run` leer, Sperre von `check.sh` frei). Bestandsaufnahme (Subagent, Sonnet, rund 7 min),
  Referenzbilder (28, `shots.sh`, ohne Problem), dieser Auftrag. Vorlagen V1–V5 an Daniel.
- 22.11.: **Daniel hat V1–V5 wie empfohlen entschieden** (Entscheidung 20 in Abschnitt 7). Kein Block ist mehr gesperrt.
  Nächster Schritt: Block A (Dateikollision) in frischer Sitzung.
- 22.11.: **Block A fertig** (Commit `7eea561`, nicht installiert). Aufgeräumt vorher: Sitzung W6-0 „done“, keine
  Worker im Repo dictate (die des Data Room nicht angefasst), keine Testumgebung. Zuerst der rote Test
  `a_chosen_target_leaves_the_noctalia_color_file_alone` (`tests/daemon_palette.rs`): gescheitert wie erwartet, die
  Wahl hatte die Farbdatei durch die Häufigkeit ersetzt. Dann `frecency::FILE_NAME` = `frecency.json` (benutzt von
  `default_path` und `daemon.rs`), `frecency::adopt_legacy` beim Start des Daemons vor `Palette::new` (Lesart L48),
  Haken `Ctl::state_files` im Harness für Dateien vor dem Start. Tests: Übernahme byte-gleich mit 0600, auch bei
  ausgeschalteter Häufigkeit, Farbdatei bleibt (auch neben `frecency.json`), beide vorhanden = nichts, strenge
  Erkennung (13 Formen, darunter gemischt mit Farbrolle oder `mode`), Link, Ordner, FIFO, zu groß. Doku: README (182,
  358), CLAUDE.md (Stand, Fallen/Design), Modulkopf `frecency.rs`, `daemon.rs`, `config.rs`, GUI-Text
  `pages/palette.rs` 222. Prüfung `scripts/check.sh --only fmt,clippy,unit,gui` grün (579 s: unit 465, gui 95);
  **Abweichung:** `gui` zusätzlich zum Auftrag, weil der sichtbare Text in `src/gui/pages/palette.rs` mitging (Tabelle
  in CLAUDE.md). Dauer **gemessen** vom ersten Build (roter Test) bis zum Stand-Commit 18 min, davon `check.sh` 579 s;
  Aufräumen und Lesen davor nicht gemessen. Nachtrag aus der eigenen Durchsicht (Commit nach `ec05192`): der
  Deskriptor von `palette.json` blieb beim Inode-Vergleich nicht offen (`file.take` verbrauchte ihn, die Nummer hätte
  neu vergeben werden können); jetzt offen bis nach dem Entfernen, dazu Länge und `mtime` am Deskriptor (Überschreiben
  an Ort und Stelle). Der Weg `MovedButReplaced` ist über die Naht `adopt_legacy_with` getestet (`rename` und
  Überschreiben während der Übernahme). Geprüft: `--lib palette::sources::frecency` (25), `--test daemon_palette` (22),
  `fmt,clippy`. **Offen (Frage an Daniel):** Liegen beide Dateien vor und löscht Daniel die Häufigkeit
  (`pal_frecency_clear`), holt der nächste Start die alte `palette.json` als Häufigkeit zurück, falls Noctalia sie bis
  dahin nicht überschrieben hat; nur möglich, wenn eine alte Häufigkeit und `frecency.json` zugleich entstehen (auf
  blub nicht zu erwarten). Nicht nebenbei gebaut; mögliche Lösung: `clear` entfernt auch eine `palette.json`, die
  `is_frecency_doc` erfüllt. Nächster Schritt: Block B (beginnt mit Messpunkt M1) in frischer Sitzung.
- 22.11.: **Block B fertig** (Test-Infrastruktur, nicht installiert; Commit `b0082b6`).
  Aufgeräumt vorher: Sitzung Block A „done“, keine Worker im Repo dictate (die des Data Room nicht angefasst, Chat-Tab
  kann die CLI nicht schließen), keine Testumgebung, Sperre von `check.sh` frei. **M1** zuerst (Ergebnis oben unter
  Messpunkte, Lesart L41 geändert). Gebaut: Lage-Log `src/gui/layout_log.rs` (L46; nur mit `DICTATE_DEBUG`, Zeile
  `dictate-settings: layout <id> x y w h visible|hidden` bzw. `<id> gone`, nur Änderungen je Frame, `visible` = ganz
  im Ausschnitt; IDs fest: `nav:<seite>` über neues `Page::arg` (auch von `from_arg` benutzt), `apply`, `revert`,
  `feature_switch`, `ai.timeout_ms`, `diagnose.add_target`, `diagnose.ping_targets[i].name|address|remove`; Diff als
  reine Funktion mit zwei Unit-Tests). `tests/gui/lib.sh`: `gui_spot`, `gui_target` (sichtbar und ruhend, sonst FAIL),
  `gclick_id`, `gui_reveal` (scrollt in 3er-Schritten); `gui_start` setzt `DICTATE_DEBUG=1` nur für das Fenster;
  `gui_fixtures` nach `nested_up`: `fixtures/gtk-4.0/settings.ini` (`Noto Sans 12`, das, was blub wegen des fehlenden
  Roboto tatsächlich zeigt; FAIL, wenn `fc-match` eine andere Familie liefert) nach `$XDG_CONFIG_HOME/gtk-4.0/`,
  `fixtures/palette-dark.json`/`palette-light.json` (Tokyo Night aus niri-coach `c123612`, alle 28 Rollen in der
  Reihenfolge der Vorlage) als Kopie nach `$NESTED_DIR/palette.json` = `DICTATE_PALETTE` (`GUI_PALETTE=light`). Der
  Harness (`harness.sh`, auch Schritt `wayland`) ist unverändert. `smoke.sh` (About ausdrücklich ohne Apply,
  fehlendes Ziel = Fehler), `diagnose.sh` (neu: Prüfung „third row is gone“ aus dem Log), `interact.sh` klicken nur
  noch über IDs; feste Rasten nur noch für Bildausschnitte. **Eichung vor dem Umstellen** (Wegwerfskript): alle neun
  alten Klickpunkte lagen in den geloggten Rechtecken (z. B. Apply 332,824 in 288,804.9 88.4x38.8), Ursprung und
  Skalierung stimmen. `interact.sh` (nicht in `check.sh`) einmal von Hand: 0 Probleme, Mock bekam erst Ablehnung bei
  `ai.timeout_ms`, dann „ok“, Bilder `i-ai-error`/`i-ai-applied` selbst angesehen (Fehler rot am Feld, dann
  „Applied.“). **Prüfung:** `scripts/check.sh --only fmt,clippy,gui` grün (123 s: clippy 18, gui 103), `cargo test
  --release --bin dictate-settings` 164 bestanden, `--lib palette::intent` 13 bestanden (weil `src/gui/mod.rs` als
  Quelltext gelesen wird). Dauer **gemessen** vom Laden der Orca-CLI (13:15) bis zur grünen Prüfung (13:33) rund 18 min,
  davon Prüfung rund 3,5 min. **Abweichungen:** keine Schrift- oder Paletten-Prüfung im Fenster selbst, weil
  `dictate-settings` beides erst ab Block C liest (die Fixtures wirken bis dahin nur auf GTK-Programme im Harness);
  ein Test, dass die dunkle Fixture `ui::palette::FALLBACK` entspricht, gehört zu Block C (Regel 2: wer
  `src/ui/palette.rs` ändert, prüft `wayland`). Nächster Schritt: Block C (Thema zur Laufzeit) in frischer Sitzung.
- 22.11.: **Block C, Teil 1 von 2** (Thema aus Rollen, nicht installiert; Commit `dc5a9d5`). Aufgeräumt vorher:
  Sitzung Block B „done“, keine Worker im Repo dictate (die des Data Room nicht angefasst, Chat-Tab kann die CLI nicht
  schließen), keine Testumgebung, Sperre von `check.sh` frei. **Schnitt:** Teil 1 = Messung „vorher“ (M3, M4), Rollen,
  `#[path]`, `RADIUS`, Thema aus Rollen mit V2 und Ecken L42, Kontrasttest und M5, Pixeltest beim Start; Teil 2 =
  Beobachter L43, Tausch im laufenden Fenster, M2, M3/M4 „nachher“, Schrift L41, Typografie L40.
  **Messung vorher** (unverändertes Binary von `b0082b6`, gesichert als
  `/home/data/stt-test/logs/w6-c/dictate-settings-before`; Skript `measure.sh` daneben, Instrument `WAYLAND_DEBUG=1`
  des Fensters im verschachtelten niri gegen den Mock, Seite General, Maus ruhig): **M3** 0 `wl_surface.commit` in
  10 s Leerlauf (drei Läufe); **M4** erste Wayland-Nachricht bis erster Commit nach dem ersten `attach` (enthält
  `App::new` mit Schriftsuche und den ersten Frame) 466, 338, 364 ms. Gebaut: `src/ui/palette.rs` liest vier Rollen
  mehr (`on_primary`, `surface_container_low`, `surface_container`, `surface_container_high`; zusammen zehn, Liste
  `ROLES`), `RADIUS` (12) zog aus `view::size` hierher, dazu `RADIUS_CONTROL` (8) und `RADIUS_CHIP` (4) nach L42.
  Tests: `FALLBACK` gleich `tests/gui/fixtures/palette-dark.json` (per `include_str!`), jede Rolle aus `ROLES` steht in
  der Vorlage und ist Pflicht, beide Fixtures tragen genau die 29 Schlüssel der Vorlage in ihrer Reihenfolge; die
  Paletten der Wayland-Tests (`tests/wayland/palette-tokyo.json`) haben alle 28 Rollen, kippen also nicht auf den
  Rückfall. `dictate-settings` bindet `src/ui/` per `#[path = "../ui"] mod ui { fsutil, palette }` ein (keine Kopie).
  `src/gui/theme.rs`: `Theme` aus den Rollen (Fläche `surface`, Seitenleiste `surface_container_low`, Popups und
  Hinweisleiste `surface_container`, Felder `surface_container_high`, Text `on_surface`, Nebentext
  `on_surface_variant`, Akzent `primary`, Text auf Akzent `on_primary`, Linien `outline_variant`), aktuelles Thema je
  Thread (`th()`, wie `layout_log`), `apply` setzt `ctx.set_theme` fest aus `mode` (egui 0.37 folgt sonst mit
  `ThemePreference::System` dem Systemthema: Regel 3) und `set_style_of` mit `Visuals::dark()`/`light()` als Grund;
  Ecken nach L42 (Felder, Knöpfe, Zeilen 8, Chips 4, Popups 12). Die 79 Stellen lesen `th()`; die Zeilen der
  Seitenleiste wie `.dp-row` (12 px eingerückt, Ecken 8, aktiv und Hover Akzent 16 %). Beim Start liest das Fenster
  die Palette (`theme::load`, Zeile `dictate-settings: palette from … (dark|light)` bzw. Rückfall mit Grund).
  `Theme::text_pairs` nennt jede Paarung von Text und Fläche mit der tatsächlich gemischten Fläche (16 Paare,
  abgeschaltete Bedienelemente ausgenommen). **M5** (Test `text_pairs_have_contrast`, Werte in
  `/home/data/stt-test/logs/w6-c/m5-kontrast.txt`): Tokyo Night dunkel alle über 4,5:1 (kleinster Wert 5,25, Text auf
  markiertem Text im Feld); Tokyo Night hell nur „Text auf `surface`“ (4,52), die übrigen 15 darunter (z. B. Nebentext
  auf `surface` 3,57, Akzent auf `surface` 3,11, `on_primary` auf `primary` 3,11, Platzhalter im Feld 2,82). Das liegt
  an der Palette selbst; der Test hält die 15 Paare als Liste `LIGHT_BELOW` fest (keine neue Paarung kommt unbemerkt
  dazu), bis Daniel die Frage unten beantwortet. Neuer GUI-Test `tests/gui/theme.sh` (im Schritt `gui`): Start mit
  der dunklen Kopie, Pixel der Seitenleiste = `surface_container_low` und des Inhalts = `surface` (Erwartung aus dem
  JSON des Laufs), Logzeile; dann Kopie per `rename` durch die helle ersetzt, Neustart, dieselben Prüfungen hell.
  Beide Pixel exakt (`#1f2130`/`#1a1b26`, `#d8dbe5`/`#e1e2e7`), Bilder selbst angesehen
  (`/home/data/stt-test/logs/w6-c/gui/theme-dark.png`, `theme-light.png`: stimmig, Ecken rund, hell lesbar, aber
  sichtbar kontrastarm). **Prüfung:** `scripts/check.sh --only fmt,clippy,unit,gui,wayland` grün (1227 s:
  clippy 24, unit 613, wayland 480, gui 108, darin `theme.sh` ok), dazu `cargo test --release --bin dictate-settings`
  172 bestanden, `--bin dictate-ui` 361, `--lib ui::` 196 (darin `ui::palette` 4), `--lib palette::sources::frecency`
  25. Dauer **gemessen** vom Laden der Orca-CLI (13:38) bis zur grünen Prüfung (14:16) rund 38 min, davon
  Prüfung 20,5 min und Messung vorher rund 3 min. Code-Commit `dc5a9d5`.
  **Abweichungen:** das Glühen des Hauptknopfs (Pushing-Pixels-Element) bleibt vorerst, nur mit runden Ecken (gehört
  zu Block D: Seiten); `src/bin/dictate-ui/panel.rs` schreibt die 12 im CSS noch als Zahl statt `RADIUS` (nicht
  angefasst, nur vermerkt); Schrift, Versalien und Laufweite unverändert (Teil 2). Nachtrag nach Durchsicht der
  Bilder aller Seiten aus dem Prüflauf (`/home/data/stt-test/logs/w6-c/sheet-teil1.png`: Audio, Model, Overlay,
  About, Diagnose mit Fehlern, AI; stimmig): der Text der Seite Audio sagte „turn teal“, jetzt „take the accent
  color“ (Prüfung `fmt,clippy,gui` grün, 165 s). Noch „teal“ für Block D: `src/bin/dictate-ui/diag_view.rs` 519
  (sichtbarer Text im Diagnose-Panel „Teal names such as user-1 …“, falsch seit W1) und README 242.
  **Frage an Daniel (Kontrast, Regel 5; sperrt den Abschluss von C, nicht Teil 2):** Mit Tokyo Night hell erreicht
  kaum eine Textpaarung 4,5:1, weil die Palette selbst so schwach ist (Werte oben). (a) **Empfehlung:** das Fenster
  schärft eine Textfarbe nur dort nach, wo sie auf ihrer Fläche unter 4,5:1 liegt: in kleinen Schritten zu Schwarz
  (hell) bzw. Weiß (dunkel), bis 4,5:1 erreicht sind; der Farbton bleibt, Tokyo Night dunkel bleibt unverändert; eine
  Stelle (`Theme::from_palette`), der Test prüft dann beide Paletten auf 4,5:1. (b) Die Palette gilt, wie sie ist;
  Regel 5 nur für dunkle Schemata, hell wird nur gemessen. (c) Für Akzenttext die Container-Rollen
  (`on_primary_container` auf `primary_container`, hell 4,52); hilft nicht beim Nebentext. Dieselbe Schwäche hat
  `dictate-ui` (Nebentext im Overlay und Panel hell 3,57): bei (a) eigener kleiner Schritt danach, nicht in W6
  nebenbei. **Beantwortet am 22.11. (Entscheidung 21):** (b), die Palette gilt, wie sie ist.
- 22.11.: **Block C, Teil 2 von 2, damit Block C fertig** (nicht installiert; Commits `d2a6b3d` Nachtrag Block A,
  `29b4fc0` Code). Aufgeräumt vorher: Sitzung Teil 1 „done“, keine Worker im Repo dictate (die des Data Room nicht
  angefasst, Chat-Tab kann die CLI nicht schließen), keine Testumgebung, Sperre frei. Daniels Antworten auf alle
  offenen Fragen kamen während der Sitzung (Entscheidung 21) und sind umgesetzt: Regel 5 und Kontrasttest (dunkel
  4,5:1, hell nur Ausgabe, `LIGHT_BELOW` entfällt), L49 bestätigt, M1 berichtigt (Roboto war ein Messfehler; Nachtrag
  in `w6-m1/ergebnis.md`), Fixture `Roboto 12`, Clear (`frecency::remove_legacy`, gemeinsames Lesen `read_legacy`
  mit `adopt_legacy`; Tests: beide vorhanden → Clear löscht beide, `adopt_legacy` danach `Absent`; Farbdatei, Link und
  fremder Dateiname unberührt; Ersetzen und Überschreiben während des Löschens; Daemon-Test über `pal_frecency_clear`).
  **Nachmessung Größe** vor dem Übernehmen (unter Messpunkte, M1): 12 pt = 16 px, `gtk-xft-dpi` wirkt, Gewichtswörter
  nicht. Gebaut: `src/gui/fonts.rs` (einzige Quelle `gtk_font`: `settings.ini` wie GKeyFile, `gtk-font-name` wie Pango
  zerlegt, nur die Familie an `fc-match`, Größe 8 bis 32 px begrenzt; Flächen mit `%{index}`: untere 16 Bit Fläche der
  Sammlung, obere benannte Instanz einer variablen Datei; variable Dateien bekommen `wght` 400/600, statische nehmen die
  Datei, die fontconfig für `semibold` nennt; jede Datei vorher mit skrifa geprüft, weil egui an einer unlesbaren mit
  panic abbricht; `skrifa` 0.44 als direkte Abhängigkeit, steht über egui schon im Lockfile). `src/gui/watch.rs`
  (Lesart L43/L51: inotify auf den Ordner, nur der Dateiname, 50 ms Entprellung mit dem Zeitpunkt des ersten
  Ereignisses, fehlender Ordner über den nächsten Vorfahren, nach jedem Scharfstellen einmal wecken, Rückfall mtime im
  Thread). `theme::Live` (Beobachter vor dem ersten Laden, neu gestylt nur bei anderer Palette, Zeile „theme: palette
  applied after N ms“ erst aus dem Durchlauf, dessen Wurzel-`Ui` den neuen Stil trägt: eframe legt sie vor `logic`
  an). Typografie `theme::Size` nach L40/L50, `label_caps` und `fam_label` entfallen; die Punkte der Listen auf den
  Seiten Palette und Diagnose richten sich nach der Zeilenhöhe der Schrift statt fester 20 px. Log beim Start:
  `font Roboto 16.00 px from …/settings.ini (base Roboto-Regular.ttf, title Roboto-Bold.ttf)`.
  `tests/gui/theme.sh`: Start dunkel, Schriftzeile, fremde Datei im Ordner (`frecency.json`) ändert nichts, Tausch per
  `rename` im laufenden Fenster färbt um (Pixel mit Wiederholung bis 3 s), Löschen fällt auf Tokyo Night dunkel zurück,
  Neustart hell. Unit-Tests: Zerlegen wie Pango (Entscheidung 21), Muster maskiert, GKeyFile, `%{index}`, unlesbare
  Schrift abgelehnt; Beobachter (Ersetzen, fremde Namen, Löschen, Entprellen, fehlender und wiederkehrender Ordner).
  **Messung:** **M2** 77 bis 78 ms (Tausch und Löschen, drei Läufe von `theme.sh`, davon 50 ms Entprellung; erster
  Lauf vor dem Parallelstart 100/97 ms). **M3** nachher 0 Commits in 10 s in allen drei Läufen (wie vorher).
  **M4** zuerst 468/504/426 ms (Median 468 gegen vorher 364, Streuung vorher 338 bis 466): jeder `fc-match` kostet auf
  blub rund 115 ms, und es waren vier nacheinander statt drei; danach die vier gleichzeitig (`fonts::resolve_all`):
  243/216/210 ms, Median 216 (rund 150 ms schneller als vorher). Zwei Läufe statt einem, weil der Code nach dem ersten
  geändert wurde; beide in `measure.tsv`. **M5** dunkel unverändert alle über 4,5:1. **Bilder** selbst angesehen
  (`/home/data/stt-test/logs/w6-c/gui-t2/`, `shots-t2/`): General dunkel und hell (live umgefärbt), hell mit offenem
  Dropdown (Popup `surface_container`, Ecken 12, gewählte Zeile Akzent, lesbar), AI dunkel und Palette hell: stimmig;
  Knöpfe zuerst in 600, zu schwer neben den Chips, jetzt 400 (L50). **Prüfung:** `scripts/check.sh --only
  fmt,clippy,unit,gui,wayland`: fmt, clippy, unit (302 s) grün, `wayland` im ersten Lauf 528 ok, 11 FAIL, alle in
  `palette.sh` (Fälle „own clipboard“ ab „copy closes the panel“ und „text action“ ab „the daemon got PRIMARY
  exactly“, Folgefehler eines nicht geschlossenen Panels), `gui` lief deshalb nicht; Wiederholung `--only wayland,gui`
  grün (594 s: wayland 487, gui 107). Kein Bezug zu den Änderungen (`dictate-ui`, `src/ui/` und Zwischenablage
  unverändert); als Wackeln vermerkt, das Log der betroffenen Instanz war vom Sperrtest überschrieben. Dazu `cargo test
  --release --bin dictate-settings` 182 (vorher 172), `--bin dictate-ui` 361, `--lib ui::palette` 4,
  `--lib palette::intent` 13, `--lib pipeline::commands` 15, `--lib palette::sources::frecency` 27 (vorher 25),
  `--test daemon_palette` 23 (vorher 22). Dauer **gemessen** vom Laden der Orca-CLI (14:22) bis zur grünen Wiederholung
  (15:20) rund 58 min, davon Prüfung rund 24 min. **Abweichungen:** L51 (Rückfall im Thread statt
  `request_repaint_after`); Gruppenlabels ohne die Laufweite von `.dp-group` (L50); Wortmarke „DICTATE“ bleibt bis
  Block D in Versalien. **Hinweis für Handläufe:** `cargo test --bin dictate-settings` baut
  `target/release/dictate-settings` nicht neu (`check.sh` baut selbst). Nachtrag aus der Durchsicht (Commit `efadde9`): die
  Entprellung wartete ab jedem Ereignis im Ordner, Schreibvorgänge anderer Dateien unter 50 ms Abstand (Logs im
  Harness, Daemon) hätten das Wecken unbegrenzt verschoben; jetzt zählt die Ruhezeit ab dem letzten passenden
  Ereignis (Test `other_files_do_not_starve_the_debounce`), dazu der Zeit-Assert in `bursts_are_debounced` lastfest.
  Geprüft: `--bin dictate-settings` 183, `scripts/check.sh --only fmt,clippy,gui` grün (155 s, M2 75/77 ms). Für
  Block E (Code-Durchsicht): `watch.rs` (unsafe inotify), `fonts.rs`, `frecency::remove_legacy` (entfernt eine Datei
  neben der von Noctalia). Fallen die Fälle „own clipboard“/„text action“ in `palette.sh` erneut, als Befund
  behandeln und das Log der Instanz sichern, bevor der Sperrtest es überschreibt. Nächster Schritt: Block D in
  frischer Sitzung.
- 22.11.: **Block D fertig** (Seiten, Logo, Texte; nicht installiert; Code-Commit `f501a56`).
  Aufgeräumt vorher: Sitzung Block C Teil 2 zuerst noch „working“, beim zweiten Blick geschlossen (`agent show` ohne
  Ort); keine Worker im Repo dictate (die des Data Room nicht angefasst, Chat-Tab kann die CLI nicht schließen), keine
  Testumgebung, Sperre frei. Gebaut: **Overlay-Vorschau** nach L45/L52: `src/ui/wave.rs` (neu) trägt Maße, Welle, Töne,
  ihre Deckkraft (`OFF_ALPHA`, `DASH_ALPHA`) und den Zellradius, ohne `protocol`; `view.rs` reicht es weiter,
  `dictate-ui` zeichnet die Zellen jetzt mit diesen Konstanten statt Zahlen, `dictate-settings` bindet `wave.rs` per
  `#[path]` ein. Die Seite zeichnet Bildschirm (1920x1080 angenommen) und Overlay im selben Maßstab (bei 720 Breite
  0,375): Karte `surface` mit eingestellter Deckkraft, Rahmen `outline_variant`, Ecken `RADIUS`, Welle 7x5 mit festen
  mittleren Pegeln, Text als Balken (`on_surface`, Tail `on_surface_variant`), oben/unten mit Abstand; Unit-Tests der
  Lage und der Welle. **Logo** nach V3 (`widgets::logo`, `GLYPH`, `lockup`: Zellen 12 %/50 % `on_surface`, Akzentzelle
  `primary`, Wortmarke „dictate“ in der Titelschrift in den Verhältnissen des Lockups; Test `logo_follows_the_svg`
  gegen `glyph-on-dark.svg` und `lockup-on-dark.svg`), Seitenleiste 16 px statt „DICTATE“ in Versalien, About 36 px.
  **About** nach V4 (Name, „Pushing Pixels, Vienna“, Link; Bild und Texturen entfallen, `App::lockup`/`pp_glyph`
  entfernt). **Texte:** Seite Palette „Mod+Alt+Space opens the panel. Keys in the panel:“ statt „no panel and no key
  yet“; General nennt alle sieben Kürzel (`SHORTCUTS`, Test `shortcuts_match_the_kdl` vergleicht Menge und Reihenfolge
  mit dem `binds`-Block); Diagnose-Panel „Names in the accent color such as user-1 …“ statt „Teal names …“ (Kommentar
  `marked` mit Rollen), README (Vorschau: Fehler- und Akzentfarbe, Kopf „Sends to“/„Runs on“ statt „Teal-Pixel“);
  Kommentar am Link auf About ohne Bezug zum Guide. **Glühen** entfernt (L53: Hauptknopf mit Zustandsschicht
  `on_primary` 8 % beim Hover, neues Paar im Kontrasttest; ebenso die Punkte „Daemon running“ und Seite AI).
  **Fensterregel** `geometry-corner-radius 12` und `clip-to-geometry true` für `dictate-settings` in
  `niri/dictate-binds.kdl` (`niri validate` gültig; Test `window_rule_rounds_the_settings_window` gegen `RADIUS`).
  Mitgenommen: `src/bin/dictate-ui/panel.rs` schreibt die Ecken im CSS aus `RADIUS`, `RADIUS_CONTROL`, `RADIUS_CHIP`
  statt 12/8/4 (vermerkt in Teil 1). **Bilder** (`tests/gui/shots.sh`, dunkel und `GUI_PALETTE=light`, 56 Bilder,
  0 Probleme; `/home/data/stt-test/logs/w6-d/shots/`, Kontaktbögen `sheet-dark.png`/`sheet-light.png`) selbst
  angesehen: niri rundet das Fenster tatsächlich (alle vier Ecken, nichts abgeschnitten); vorher/nachher-Paare mit
  den Referenzbildern `w6-ref/before-*` für Overlay, General, AI und About (`cmp1.png`, `cmp2.png`): Aufbau gleich,
  geändert nur Vorschau, Logo, About und (seit Block C) Farben, Schrift, Versalien; alle 14 Seiten hell in halber
  Größe (`half-light-*.png`): lesbar, nichts abgeschnitten, Kontrast so schwach wie die Palette (Entscheidung 21);
  Logo und About in beiden Modi, General mit sieben Kürzeln. Der neue Text der Gruppe „Keys“ auf der Seite Palette
  steht in keinem Bild (liegt unter dem zweiten Ausschnitt), geprüft nur im Code. Befund am eigenen
  Bild: die Welle der Vorschau war mit lauten Pegeln ein voller Block; jetzt mittlere Pegel (Säulen mit 3 und 5 Zellen,
  `zoom-overlay-b.png`), Test `sample_wave_looks_like_speech`. **Prüfung:** `scripts/check.sh --only
  fmt,clippy,gui,wayland` grün (659 s: wayland 543, gui 112; `palette.sh` diesmal ohne Wackeln), dazu `cargo test
  --release --bin dictate-settings` 189 (vorher 183), `--bin dictate-ui` 361, `--lib ui::` 196, `--lib palette::intent`
  13, `--lib pipeline::commands` 15. Dauer **gemessen** vom Laden der Orca-CLI (15:28) bis zur grünen Prüfung (16:01)
  rund 33 min, davon Prüfung 11 min. **Abweichungen:** Punkte ohne Glühen über den Auftrag hinaus (L53, gleiches
  Kriterium wie der Knopf); `panel.rs` auch 8/4 aus den Konstanten, nicht nur 12. Die PNGs unter `src/gui/assets/`
  sind ab jetzt unbenutzt und bleiben bis Block E liegen (Zuschnitt: Löschen nach V5; das Paket `image` bleibt für
  das Fenster-Icon). **Berichtigung zum Wackeln aus Teil 2:** das Log ging nicht verloren; jeder Lauf von `check.sh`
  hat einen eigenen Ordner (`latest` ist nur ein Link), und `lock.sh` sichert die Instanz von `palette.sh` vor dem
  Neustart als `ui-actions.log`. Das Log des Laufs mit 11 FAIL liegt unter
  `/home/data/stt-test/logs/check/20261122-145607/wayland/ui-actions.log` (nicht ausgewertet; ebenso ein Lauf vom
  Vormittag, `20261122-102820`, 3 FAIL). **Prüfhinweis:** `src/ui/wave.rs` speist jetzt auch das Einstellungsfenster;
  wer es ändert, prüft `wayland` **und** `gui` sowie `--bin dictate-settings` (in der Tabelle in CLAUDE.md
  nachgetragen). Nächster Schritt: Block E in frischer Sitzung.
- 22.11.: **Block E fertig** (Abschluss; nicht installiert, nicht gemergt, nicht gepusht; Commits `c585201` Doku,
  `817f66b` Testsynchronisation, `741c2fb` und `a02af8e` Befunde der Durchsicht, dazu dieser Stand).
  Aufgeräumt vorher: Sitzung Block D „done“, keine Worker im Repo dictate (die laufenden des Data Room nicht angefasst,
  Chat-Tab kann die CLI nicht schließen), keine Testumgebung, Sperre frei. **Doku nach V5:** `DESIGN.md` neu gefasst
  aus dem Code (Palette und Rückfall, Tabelle Rolle → Verwendung in `dictate-ui` und im Fenster, Kontrast nach
  Entscheidung 21, Schrift L41/L50, Ecken L42, 8-px-Raster, Zustände ohne Glühen L53, Overlay, Panels, Fenster mit
  Chips und Gruppen, Overlay-Vorschau L52, Logo V3/L53, Marke V4, Textregeln); dabei vermerkt: der Hauptknopf im Panel
  setzt Text in `surface` auf `primary`, das Fenster in `on_primary` (bei Tokyo Night gleich, offen). Gelöscht:
  `design/pushingpixels/`, `assets/brand/`, `src/gui/assets/` (`image` bleibt für `assets/logo/icon-512.png`). README
  (Einstellungsfenster in Noctalia-Farben und GTK-Schrift, Palette-Datei, `frecency.json`, Fensterregel und Binary
  zusammen, Hinweis auf `assets/brand/` entfernt), ROADMAP, CLAUDE.md (Design, Fallen, Prüftabelle mit `--bin
  dictate-settings`, veralteter Satz zu festen Klickzielen). Historische Pläne mit Archiv-Vermerk (`SPEC-v2.md`,
  `PLAN-actions.md`, `UX-actions.md`, `PLAN-palette.md`, `docs/w7-awesome-inventar.md`) nennen `DESIGN.md` noch mit dem
  alten Inhalt; nicht umgeschrieben. **Wackeln in `palette.sh` ausgewertet** (Lauf `20261122-145607`, 11 FAIL, ohne
  Inhalte gelesen): nach `act_run 10: result` (Zeile des Arbeiters) zeichnete der Hauptthread das Ergebnis (Lage
  `copy`) erst 924 ms später; das Return des Tests kam davor und verfiel in „Working“ bzw. der Totzeit, das Panel blieb
  offen; die übrigen 10 FAIL folgen daraus (der Kürzel der Palette schloss das Panel statt zu öffnen, `pal_type`
  tippte ins Auswahlfenster und nahm ihm die Markierung). Im grünen Lauf kam dieselbe Lage 778 ms nach der Zeile, also
  knapp; **wahrscheinliche Ursache, nicht bewiesen.** Korrigiert die Synchronisation: `result_shown` in
  `tests/wayland/actions.sh` wartet zusätzlich auf eine neue Lage `copy` (nur in „Result“) vor jedem Return auf ein
  Ergebnis (P16b in `palette.sh`, Schritte 22–25 in `actions.sh`). Weiter beobachten. Der Lauf `20261122-102820`
  (3 FAIL „screenshot Esc“) fiel in den Versuch zu Entscheidung 19 (10:21–10:39; **abgeleitet** nur aus den
  Commit-Zeiten) und ist demnach kein Wackeln. **Durchsicht**
  (Subagent Opus, nur lesend, Bericht `/home/data/stt-test/logs/w6-e/review.md`): 1 wichtig, 6 gering, alle am Code
  nachgeprüft. Behoben: L54 (alte Häufigkeit nur über den eigenen Deskriptor leeren statt `unlink`, Test vorher rot);
  Beobachter weckt im Vorfahren nur für den Eintrag auf dem Weg zum Ordner (Test), prüft neu statt Dauer-Rückfall,
  wenn der Ordner zwischen Prüfen und Scharfstellen verschwindet, stellt nach `IN_Q_OVERFLOW` neu scharf, Frist
  aufgerundet (Test `poll_timeout_rounds_up`); `theme::Live` loggt einen neuen Rückfallgrund auch bei gleicher Palette
  (Test). Gegenlesen des Fix-Diffs: zwei geringe Punkte, behoben (`Adoption::MovedNotEmptied` bei zweitem Namen oder
  nicht schreibbarer Datei, „Clear“ scheitert dann bewusst laut; eigenes `on_dir`; Test mit hartem Link). **Nicht
  behoben, Vorlage an Daniel:** `fc-match` läuft ohne Frist im Hauptthread (vor W6 schon so); Empfehlung: großzügige
  Frist (etwa 10 s, kalter fontconfig-Cache), danach Kinder beenden und einsammeln, M4 nachmessen. Nicht getestet:
  Überlauf und das Rennen beim Scharfstellen (nur am Code). **Prüfung:** gezielt `--lib palette::sources::frecency` 29,
  `--test daemon_palette` 23, `--bin dictate-settings` 192 (1 ignoriert); volle Suite `scripts/check.sh` grün (Lauf
  `20261122-163353`, 1461 s: unit 443, wayland 568, gui 111, speech 242 mit CER 0,0054, gpu 96; keine FAIL-Zeile in
  `wayland`), danach keine Reste (kein verschachteltes niri, `wl-run` leer, Sperre frei, keine Pulse-Module). **Echte
  `palette.json` auf blub** (nur Schlüssel gelesen): Farbdatei vom 20.11., 29 Schlüssel, `mode` dark, alle zehn
  Pflichtrollen da, keine `frecency.json`; die Installation verschiebt also nichts. Dauer **gemessen** rund 55 min
  (16:05 bis 17:00), davon Suite 24 min; Token der Hauptsitzung nicht gemessen, Durchsicht samt Gegenlesen
  rund 224k (Subagent). Abschluss: Fragen an Daniel (Installation mit KDL,
  Merge, Push); keine neue Sitzung (Kette endet mit W6).
- 22.11.: **Nach Block E** (Entscheidung 22, Daniel): gemergt nach `main` (`5063f36`, ohne Vorspulen), installiert
  (alle drei Binaries aus dem Worktree mit `--target-dir target`, Baum gleich `main`; beide Dienste neu, `active`;
  `dictate-ui` lädt die Farbdatei, dunkel; `palette.json` unverändert vom 20.11., keine `frecency.json`), KDL über
  chezmoi (`niri validate` gültig, nur diese Datei angewandt, chezmoi-Commit `50bf273`, nicht gepusht; die übrigen
  Einträge in `chezmoi status` nicht angefasst), `main` und `wayland-w6` nach `origin` gepusht. Nachzug im Branch
  `w6-nachzug`: Frist für `fc-match` (10 s) und Knopftext im Panel `on_primary`.
- 22.11.: **Nachzug nach Entscheidung 22** (Branch `w6-nachzug` von `main` `1add3e0`): (4) `fonts::resolve_all` mit
  Gesamtfrist `FC_DEADLINE` 10 s (Abfrage alle 5 ms, danach `kill` und `wait` je hängendem Aufruf, Ausgabe höchstens
  64 KiB, Ergebnis je Muster ein Fehler, Fenster mit egui-Schrift); Test `a_hanging_fc_match_is_stopped_at_the_deadline`
  mit Fake-Programm (antwortet bzw. `exec sleep`, Frist 300 ms, kein Rest). **M4** nachgemessen mit
  `/home/data/stt-test/logs/w6-c/measure.sh` (gleiches Instrument): 231/158/183 ms, Median 183 (vorher 216); **M3**
  0 Commits in 10 s. (5) Panel von `dictate-ui`: Text auf dem Hauptknopf und im angekreuzten Kästchen (`.dp-box.on`,
  gleiche Regel, mitgenommen) in `on_primary` statt `surface`; Test `text_on_the_accent_is_on_primary` mit
  unterscheidbaren Rollen. `DESIGN.md` nachgezogen (Vermerk „offen“ entfällt). **Prüfung:** `--bin dictate-ui` 362,
  `--bin dictate-settings fonts` 6, `scripts/check.sh --only fmt,clippy,gui,wayland` grün (Lauf `20261122-171454`,
  633 s: wayland 510, gui 111; keine FAIL-Zeile). Danach mit Daniels Zustimmung (einzeln abgefragt): installiert
  (alle drei Binaries vom Stand `33a58e4`, nur `dictate-ui.service` neu, beide `active`, Farbdatei geladen), nach
  `main` gemergt, `main` und `w6-nachzug` gepusht.
- **Für Daniels Live-Test (W6, wächst je Block):** Schemawechsel in Noctalia bei offenem Einstellungsfenster (hell und
  dunkel), Schrift wie in den GTK-Programmen, Ecken mit Daniels Shadern, Logo und About, Häufigkeit der Palette
  überlebt einen Schemawechsel (nach Block A). Nach Block B: M1 live nachprüfen (Schrift von `dictate-ui` folgt
  `~/.config/gtk-4.0/settings.ini`, nicht dconf; Probe unter `/home/data/stt-test/logs/w6-m1/`). Die Frage zum
  Befund M1 (Roboto) ist erledigt: Messfehler, Roboto ist installiert und wird gezeigt (Entscheidung 21); live
  nachsehen, dass Panel und Einstellungsfenster Roboto zeigen. Nach Block C Teil 1: Fenster bei Daniels Schema dunkel
  und bei einem hellen Schema (Farben exakt wie Noctalia, hell nur gemessen, Entscheidung 21), gerundete Felder und
  Zeilen. Nach Block C Teil 2: Schemawechsel in Noctalia bei offenem Fenster färbt ohne Neustart um (Zeile
  „theme: palette applied after N ms“ im Journal bzw. auf stderr, im Test 77 ms); Schrift Roboto 16 px wie in den
  GTK-Panels, Seitentitel und Gruppenlabels ohne Versalien; ein Löschen der Häufigkeit („Clear“ auf der Seite Palette)
  lässt die Farbdatei stehen. Nach Block D: Logo in Seitenleiste und About (hell und dunkel), Overlay-Vorschau
  gegen das echte Overlay (Lage oben/unten, Abstand, Breite, Deckkraft) und ihre Größe (L52: Felder darunter erst
  nach Scrollen), runde Fensterecken mit Daniels Shadern (erst
  mit der KDL über chezmoi), Hauptknopf beim Hover, Kürzelliste auf General.
- **Gesammelte Liste für Daniels Live-Test W2–W6 (Block E, 22.11.; Quellen: CLAUDE.md „Offen“ 1, 6–8 und die
  Listen in 6c–6h).** Alles am echten Desktop unter niri; Befunde daraus haben Vorrang.
  1. **Diktat und Overlay (W1/W2):** Sprachbefehle mit echter Stimme, Live-Pegel mit dem Brio, Umlaute in der Ausgabe;
     Diktat in Firefox, Alacritty, Slack und Orca (Eingabemethode T3), Chromium/Electron (Sonderzeichen über die
     eigene Tastatur); Super gehalten während der Ausgabe; Einfügen in ein XWayland-Programm bei laufendem
     Zwischenablage-Verlauf von Noctalia (Text darf nicht im Verlauf landen, alte Zwischenablage kommt zurück);
     Passwort aus dem Passwortmanager in der Zwischenablage (wird getippt, nicht eingefügt); Dauer der niri-Abfrage
     (`journalctl --user -u dictate-ui | grep 'ms to decide'`, verschachtelt 26–313 ms).
  2. **Textaktionen (W3, `Mod+I`):** echtes PRIMARY, Panel, Copy, Replace (nie in Terminals), Cloud-Vorschau mit
     Claude, Sperre über Noctalia (Panel zu, Inhalte gelöscht, `Mod+I` verweigert); beim Streaming rückt das Panel
     schrittweise hoch: stört das Wandern unter dem Zeiger?
  3. **Diagnose (W4, `Mod+Shift+I`):** Panel, Feld „What is wrong?“, Aufruf mit dem echten Schlüssel, Lage bei langer
     Antwort (rückt hoch); Hinweis im Overlay beim nächsten echten unsauberen Boot-Ende (Klick öffnet).
  4. **Befehlspalette (W5, `Mod+Alt+Space`):** Absichten aus der Tabelle in 6g; Screenshots `full`/`area` aus der
     Palette (Log „panel: transparent after N ms“ bzw. „panel not repainted in time“; steht immer Letzteres, kostet es
     300 ms je Aufnahme), Bild mit Daniels Shadern ohne Ring und Schatten; Startweg `Spawn` (Programme in
     `app-niri-<name>-<pid>.scope`); `emoji` über rofi-emoji mit `wtype`. Nicht belegt: mehrere Monitore,
     fullscreen/maximized auf das fokussierte Fenster, Sperre im Abgang.
  5. **Einstellungsfenster (W6):** gegen den echten Daemon; dunkel mit Daniels Schema und nach einem Wechsel auf ein
     helles Schema in Noctalia bei offenem Fenster (färbt ohne Neustart um, Zeile „theme: palette applied after N ms“
     auf stderr bzw. im Journal; Farben exakt wie Noctalia, hell nur gemessen); Schrift Roboto 16 px wie Panel und
     GTK-Programme (M1 live: folgt `~/.config/gtk-4.0/settings.ini`, nicht dconf); Seitentitel und Gruppenlabels
     ohne Versalien; gerundete Felder, Zeilen, Chips; runde Fensterecken mit Daniels Shadern (erst mit der KDL über
     chezmoi); Logo in Seitenleiste und About (hell und dunkel); Hauptknopf beim Hover (ohne Glühen); Kürzelliste auf
     General (sieben); Overlay-Vorschau gegen das echte Overlay (Lage oben/unten, Abstand, Breite, Deckkraft) und ihre
     Größe (L52: 405 hoch, Felder darunter erst nach Scrollen).
  6. **Häufigkeit (W6 Block A):** eine Wahl in der Palette überlebt einen Schemawechsel in Noctalia (`frecency.json`
     bleibt, `palette.json` bleibt die Farbdatei); „Clear“ auf der Seite Palette lässt die Farbdatei stehen.
- **Live-Abnahme W2–W6 (Daniel, 23.11., Entscheidung 23).** Daniel hat die installierten Stände (alle Binaries
  vom Stand `w6-nachzug`, KDL über chezmoi) am echten Desktop unter niri benutzt; Aussage: „ansonsten fühlt sich alles
  korrekt an“. Eingetragen als Live-Abnahme von W2–W6 nach der gesammelten Liste oben; Einzelnachweise je Punkt
  liegen nicht vor.
  - **Befund und Lösung:** `Mod+Alt+Space` löste neben der Befehlspalette auch die Suche von espanso aus (Vorgabe
    `ALT+SPACE`). espanso 2.4 liest die Tastatur selbst über evdev, also vor niri; die Bindung in
    `niri/dictate-binds.kdl` konnte das nicht verhindern.
    Lösung außerhalb des Repos (Daniels Wahl, die Taste von dictate bleibt): `search_shortcut: off` in
    `~/.config/espanso/config/default.yml` (die Datei verwaltet chezmoi nicht; Sicherung der alten unter
    `/home/data/stt-test/logs/w6-e/espanso-default.yml.bak`). Für neue Kürzel gilt: vorher gegen espanso (`toggle_key`,
    `search_shortcut`) und die Kürzel von niri-coach (`coach-binds*.kdl`) prüfen.
  - **Nicht belegt, beobachten** (konnte im Live-Test nicht geprüft werden oder braucht Zeit):
    1. Palette über mehrere Monitore; fullscreen/maximized auf das fokussierte Fenster.
    2. Sperre im Abgang vor Screenshots und der Weg „Frame gemeldet“ vor der Frist: nur Unit-Test.
    3. Hinweis im Overlay nach einem echten unsauberen Boot-Ende (wartet auf den nächsten Fall).
    4. Schwelle „sicherer Treffer“ der Palette (`CONFIDENT_*` in `src/palette/resolve.rs`) nach einigen Wochen
       Nutzung prüfen; `ASK_FRAME` live messen.
    5. Diagnose-Messung mit Symptom (`live_eval.rs` setzt `symptom: false`) und mit 33 Proben (die Messung in
       `diagnose-ai.md` lief noch mit 34, samt `xorg_log`).
    6. Prompt-Punkte: Refine t16 knapp über 3× Eingabe (`refine-prompt-ab.md`), `to_de` „Dear Sam“ → „Lieber Sam“
       (Geschlecht unbekannt, beide Modelle).
    7. Wackeln in `tests/wayland/palette.sh`: seit Block E über `result_shown` synchronisiert (wartet auf die neue
       Lage `copy`); die Ursache (der Hauptthread zeichnete das Ergebnis erst nach dem Return des Tests) ist
       wahrscheinlich, nicht bewiesen (Auswertung in Stand W6, Block E).
    8. Sprachtest t18 (Absatz): Whisper lässt vereinzelt einen Halbsatz aus (CER über 0,09), Schwelle unverändert
       (6e, Abschluss).
- **Bei der Installation von W6:** seit Block C sind zehn Rollen Pflicht, auch für `dictate-ui` (neu: `on_primary`,
  `surface_container_low`, `surface_container`, `surface_container_high`); fehlt eine in der echten Datei, fallen
  Overlay, Panels und Fenster auf Tokyo Night zurück (die Vorlage schreibt alle 28, die Datei vom 20.11. hat sie).
  Vorher nachsehen, was `~/.local/state/dictate/palette.json` auf blub enthält. Ist es
  inzwischen eine Häufigkeit (`version`, `entries`), verschiebt die Übernahme sie nach `frecency.json`, und bis Noctalia
  die Farbdatei neu schreibt (Schemawechsel bzw. Neustart von Noctalia), gilt Tokyo Night; Daniel darauf hinweisen.

## 6b. Aufstellung der Lua-Module (W1, 20.11.2026)

Je Datei unter `awesome/dictate/` (10 764 Zeilen): was als testbarer Rust-Code ohne GTK entsteht, was in GTK neu
gezeichnet wird, was unter niri entfällt. Zeilennummern Stand `wayland-w1`.

| Datei | Zeilen | Aufgabe | Logik → Rust | Neu zeichnen in GTK | Entfällt unter niri | Block |
|---|---|---|---|---|---|---|
| `init.lua` | 1808 | Overlay, `listen`-Anbindung, Halten/Umschalten/Abbruch, Tipp-Warteschlange, Paste, Stimmenziel, Boot-Hinweis, Kürzel | Protokoll `handle_line`/`unescape`/`parse_keys` (1207, 165, 1172); Listener-Backoff 1→30 s (1308–1360); Halten `on_press`/`on_release` mit 80 ms Entprellung (1370–1407); `do_cancel`/`cancel_recording` (1451–1474); Stimmenziel `voice_arm`/`S.voice`/`voice_next` (1366, 1214–1300); Warteschlange `pump_typing`/`stop_typing`/`release_keys`/`await_paste` (863, 1128, 784, 834); `chunks`/`text_jobs`/`queue_typing`/`paste_result` (965–1100); `focus_is_terminal` (983); Welle `set_level` (350), dB→Pegel (1250); `text_markup` (538); Hinweis-Timer (680–710); Boot-Hinweis (1499) | `make_wave` (277–348); `build_ui`/`render`/`place` (455–678); `fit_markup` (555, Pango-Messung, nur wachsende Spaltenbreite) | xproperty/picom; `transparency_mode` (406); `awful.popup`/`awful.key` (`M.keys` 1569 → KDL); `once()` (1414); `cancel_seen`-Autorepeat; Gio-Pipes (727); `dictate synth`/xdotool/`release-keys`; `ruled`-Regel, `dismiss_all_popups` (`M.setup` 1514–1565) | W1 Overlay, Listener, Halten, Abbruch, Kürzel (**erledigt**: `src/ui/`, `src/bin/dictate-ui/`); W2 Warteschlange, Paste, Terminal; W3 Sperre; W4 Boot-Hinweis; W5 Stimme → Palette |
| `actions.lua` | 2888 | Panel „Text actions“ (Markierung, Ask, Explain clipboard, `text_action`), Hülle der Diagnose | Sitzungsautomat `closed/grabbing/select/working/result/error` mit `gen`/`run_gen` (465–469, `set_state` 1852, `close` 1906); `act`-Warteschlange (504–548); `parse_grab` (1952), `grab_error` (1996), `on_grab` (2017), `run_error` (2068); Strom `delta/note/diff/result/cancelled/error` in `run_action` (2082); `replace_blocker` (1060); `copy`/`copy_block`/`replace` (2225–2310); Leerlauf 300 s `arm_idle` (1833); Totzeit/`stale`-Tasten (1852–1875, 2493–2510); `on_key` (2381); Markdown → Pango `spans_markup`/`doc_items_markdown`/`marked_markup` (1377–1483); `secret_name` (70) | `make_view`/`make_doc`/`make_scrollbar` (76–456); Knöpfe, Chips, Kopf (577–930); `build_ui` (935); `render_*` (1239–1695); `anchor`/`render` (1696–1830) | `awful.keygrabber` (`open` 2511); xproperty (1009); `mousegrabber` (446); Mausrad-Bindungen; `dismiss_all_popups`-Hook (2646–2676; Ersatz für „Klick daneben“ unter GTK offen); Ziehen; Verankerung am Zielfenster (Fensterregel mittig) | W3 Panel, Replace, Copy; W5 Quellen question/clipboard/`text_action`; W4 Hülle |
| `diagnose.lua` | 2265 | Diagnose im Panel über `dictate-diag` (Profile, Einträge, Fortschritt, Vorschau, Senden, Antwort, Prüfrunden) | Aufruf-Warteschlange `pump`/`call` mit `TIMEOUTS` (38, 77–170); Zustände `d_profiles/d_items/d_progress/d_preview/d_send/d_answer/d_error` mit `set` (1271); `split_report` (361), `mark_line` (395), `valid_spans` (947), `notes_items` (1006), `parse_answer` (1735), `unit_matches` (1407), `preset_of` (1913), `on_progress` (1512), `show_error` (1292), `stop`/`back` (1635, 1890), `on_key` (2036), `boot_hint` (173) | `render_*` und Zeilenbausteine (260–350, 464–1245) | `with_line_callback`/`awesome.kill` (Prozess-API); Keygrabber-Eigenheit `key == " "` (2040) | W4 |
| `palette.lua` | 1912 | Befehlspalette: Eingabe, Resolver/KI, Verlauf, Bestätigung, Stimme im Panel | `send`/`request`/`apply` (227–348); KI `enter_asks`/`cancel_ask`/`start_ask` (350–475); `deadtime`/`remember`/`choose`/`enter_confirm` (483–602); `valid_candidate` (293); `history_step` (1295); `one_space` (1066); `voice_*` (1678–1739); `on_key` (1364); `context()` (160) | Eingabezeile, Zeilen, Fuß, `render*` (604–1290) | `awful.prompt` mit Capture/`set_text` (1486–1560); `capi.keygrabber`; `dismiss_all_popups`; Ziehen; Tags/Layouts in `context()` (neu aus niri-IPC, Schema im Daemon offen) | W5 (W1 nur die Stimme vorbereitet) |
| `palette_exec.lua` | 768 | Führt die Absicht aus (Fenster, Tags, Programme, System, Text/KI, Diagnose) | `execute`/`done` genau einmal, `on_success` nur bei Erfolg (725); `valid_argv`/`find_exe`/`is_abs_dir` (158–198); `sink_percent`, Schritt höchstens 100 % (453–500); `launch_app` (359); `window_region` (587); `window_target` ohne Namenssuche (95) | Rückmeldung (`naughty` → Overlay/Benachrichtigung) | `client.*`, `awful.tag`/Layout, sticky/ontop (331); `env -C` (200–224, in Rust `current_dir`); `pgrep xss-lock` (435); flameshot (609); clipmenu/rofi, `call_module` doorcam/claude_usage (523); `awesome.quit` (634); fest eingetragenes `alacritty` (359, offen) | W5 (Absichten ohne Gegenstück einzeln Daniel) |
| `lock.lua` | 150 | Sperre/Schlaf über logind | Filter eigene Sitzung (40); Auslöser `hint`/`lock`/`sleep` (44–83); `GetSession($XDG_SESSION_ID or "auto")` mit Pfadprüfung (85–111); nur der Erzeuger xss-lock fällt weg | – | xss-lock; lgi-Glue; Test-Bus per Adresse (`fake-logind` als Testidee behalten) | W3 (3.6, S5) |
| `markdown.lua` | 236 | Markdown-Teilmenge → Blöcke/Spans | ganz: `split_lines` (22), `fence_open`/`fence_close` (45, 62), `inline` (68), `parse` (158), `code_blocks` (226); Nummerierung wie `src/actions/markdown.rs`; `markdown_test.lua` als Vorlage | – | nichts | W3 |
| `json.lua` | 316 | JSON, `sanitize` | `J.sanitize` (54): Längenkappung (80/128/512/1000 Zeichen); UTF-8-Reparatur entfällt mit Rust-`String` | – | Kodierer/Dekodierer (serde_json) | W5 (W4 dekodiert nur) |
| `palette_url.lua` | 84 | URLs und Suchvorlage prüfen | ganz: `encode` (18), `valid_web_url` (38), `search_url` (58); `palette_url_test.lua` als Testvektoren; Doppelprüfung zum Daemon beibehalten | – | nichts | W5 |
| `style.lua` | 106 | Schriften, `markup_text` | `markup_text` (83: Maskieren plus `TEXT_SYMBOLS`; ob die Liste unter GTK nötig ist, offen: Pango-Ersatzschrift prüfen) | Schriftklassen `F`, Laufweite → CSS (Google Sans Flex) | `gears` | W1 (Overlay-Schrift) / W3 |
| `drag.lua` | 153 | Panels mit Mod+Maus ziehen | `D.clamp` (32), trivial | – | ganz (niri verschiebt schwebende Fenster selbst) | entfällt |
| `widget.lua` | 78 | Leistensymbol (5 Zellen) mit Menü | – | Noctalia-Schaltfläche (6f, 21.11.) | `wibox`/`awful.menu` | erledigt (Noctalia) |

Befunde, die beim Port nicht verloren gehen dürfen:

1. **Terminal-Entscheidung umkehren** (W2): `focus_is_terminal()` (`init.lua:983`) gibt bei unbekanntem Fokus oder
   fehlender `terminals`-Zeile `false` und es wird **eingefügt**; Regel 4.8 verlangt tippen. Vergleich ohne
   Groß/klein; Zeilenumbruch `Return` im Terminal, sonst `shift+Return` (`init.lua:1105–1112`).
2. **Abbruch und Warteschlange** (W2): `do_cancel` (`init.lua:1468`) = `clear_notice`, `stop_typing`,
   `cancel_recording`. Danach bleibt die Warteschlange belegt, bis die Tasten losgelassen sind (`release_keys`, 5 s)
   bzw. `paste` zurückgelegt hat (`await_paste`, 9 s); ein zweiter Abbruch unterbricht das nicht
   (`init.lua:784–861, 1128–1133`).
3. **Spätes `final` nach Abbruch** (W2/W5): `cancel_recording` stellt das Stimmenziel auf `discard`; ein später
   eintreffendes `final` mit Text wird verworfen (`init.lua:1293–1297`). In W1 tippt `dictate-ui` noch nichts.
4. **Halten endet in Awesome auch beim Loslassen von Super** (`init.lua:1586–1588`). W1 endet wie in 6a und S1
   entschieden nur am Loslassen von E (Super egal). **Für Daniel:** so lassen oder Super dazunehmen.
5. **Stimmenziel beim Start entscheiden** (W5): `voice_arm` (`init.lua:1366`), `--target palette` (`init.lua:245–256`);
   das Overlay bleibt dann aus, nur Fehler erscheinen dort (`init.lua:597`).
6. **Replace** (W3): `replace_blocker` (`actions.lua:1060`): nur `output=replace`, nie im Terminal, nur wenn das beim
   Öffnen gemerkte Fenster fokussiert ist; unter niri kehrt der Fokus asynchron zurück, vor dem Tippen darauf warten.
7. **Nie loggen, nie in argv** (W3–W5): Frage und Palette-JSON nur über stdin (`init.lua:724–727`,
   `actions.lua:530`, `palette.lua:230, 432`); Rückmeldungen nur mit Zahlen („Copied N characters“).
8. **Keine Shell** (W5): argv nur über `find_exe`/`valid_argv` (höchstens 128 Argumente, kein NUL,
   `palette_exec.lua:17`); URLs nach `palette_url.lua:38–58`; Ziel nur aus der Kandidaten-ID (`palette_exec.lua:95`).
   Der Xephyr-Test gegen `with_shell`/`io.popen` braucht ein Rust-Gegenstück.
9. **Totzeit in den Panels** (W3/W5): `set_state` (`actions.lua:1852`) mit 0,3 s, gehaltene Tasten `stale` bis zum
   Loslassen, Drücken binnen 50 ms nach Loslassen gilt als Wiederholung (`actions.lua:2493–2500`, `palette.lua:483–497`);
   `confirm` braucht ein zweites Return (`palette.lua:568–602`).
10. **Reihenfolge der Anfragen** (W3–W5): `gen`/`run_gen` (`actions.lua:468–469`); Palette eine laufende plus eine
    wartende Anfrage (`palette.lua:279`), Fristen 5 s/75 s (`init.lua:94–96`); `act`-Aufrufe seriell
    (`actions.lua:504`).
11. **Sperre ist mehr als `LockedHint`** (W3): `lock.lua:44–111` löst auch bei `Lock` der eigenen Sitzung und bei
    `PrepareForSleep(true)` aus und abonniert vor `GetSession`; alle drei beibehalten.
12. **Kleinigkeiten am Protokoll**: `parse_keys` lässt nur `BackSpace` zu (`init.lua:1172`); Tipp-Frist 30 s plus
    Tippdauer (`init.lua:944`); Boot-Hinweis nach 10 s, alle 20 s, höchstens 4-mal (`init.lua:1499–1511`).

## 7. Entscheidungen (Daniel, 20.11.2026)

1. **Sprache:** dictate bleibt deutsch (Doku, Pläne, Spikes, Kommentare, Commits); Ausnahme zur Regel „Repos auf
   Englisch“. Sichtbare Texte des Tools bleiben wie bisher englisch.
2. **Kürzel:** eigene `dictate-binds.kdl` per `include` (3.1).
3. **Tastenzustand:** ja, über die Gruppe `input`, nur `EVIOCGKEY` (3.2).
4. **Oberfläche:** GTK 4, unter der Bedingung, dass GLSL-Effekte möglich sind: Panels als schwebende Fenster (niri-Shader
   wirken), Overlay als Layer-Fläche mit clientseitigen Effekten (3.3).
5. **Ausgabe:** eigene virtuelle Tastatur plus Einfügen über Data-Control; Eingabemethode nach S2 je Programm (3.4).
6. **Palette:** voll auf niri portieren (3.7, W5).
7. **Awesome-Teile:** bis zur Live-Abnahme lauffähig, danach entfernen (W7).
8. **Eingabemethode (nach S2):** T3 automatisch: Eingabemethode nur für die Dauer einer Ausgabe, Commit, wenn das
   Zielfeld sie aktiviert, sonst T2; Config-Liste für Ausnahmen.

9. **Design (20.11., während W1):** Das Pushing-Pixels-Design entfällt für die ganze Anwendung. dictate richtet sich
   nach der Palette von Noctalia (wie niri, Alacritty und niri-coach): Vorlage `noctalia/dictate.json.tmpl` →
   `~/.local/state/dictate/palette.json`, `dictate-ui` beobachtet die Datei und färbt live um, ohne Datei Tokyo Night
   dunkel (`src/ui/palette.rs`). Schrift aus den GTK-Einstellungen, Ecken 12 wie Noctalias Panels. In W1 umgesetzt
   für das Overlay; Einstellungsfenster (egui), README, `DESIGN.md`, `assets/brand/` und `assets/logo/` in W6; das
   Awesome-Modul bleibt bis zu seiner Entfernung (W7) wie es ist. (Deutung von Claude: Daniel sagte „an der Palette
   von niri orientieren bzw. ein zusätzlich installiertes Paket, dessen Name mir nicht einfällt“. Dass es Noctalia
   ist, die Schrift aus GTK, Radius 12, der Rückfall Tokyo Night und W6 für das Einstellungsfenster sind Claudes
   Lesart; von Daniel am 20.11. bestätigt.)

Noch offen für später: ~~neue Taste der Palette (W5), Absichten ohne niri-Gegenstück (W5)~~: entschieden, Punkt 16.

16. **Palette unter niri (W5, 21.11.):** Daniel hat die Vorlagen E1–E12 in 6g wie empfohlen freigegeben: Taste
    Mod+Alt+Space; Tags werden Arbeitsflächen (`view_workspace`, `move_window_to_workspace`, `previous_workspace`, Zahl =
    `idx` auf dem Monitor des Zielfensters, nur vorhandene plus die leere am Ende, „tag“ bleibt Synonym, Frecency wird
    abgebildet); `sticky`, `ontop`, `claude_usage` gestrichen; `set_layout` → `column_display` (normal/tabbed); doorcam →
    `doorcam-watch --toggle`; Verlauf der Zwischenablage → `noctalia msg panel-toggle clipboard`; Emoji über rofi-emoji;
    Programme über niri `Spawn`; Terminal fest `alacritty`; `Request::parse` wird mit `from_json` ohne Werte. Die
    Lesarten L1–L8 gelten.

17. **Palette nach Block D (W5, 22.11.):** **F1** Screenshot `full` und `area` aus der Palette: das Panel wird zuerst
    unsichtbar (GTK-Deckkraft 0), dictate-ui wartet, bis GTK diesen Frame an niri übergeben hat (Frame-Uhr, höchstens
    300 ms), löst dann die Aufnahme aus und schließt das Panel erst danach (die Schließ-Animation läuft auf einem
    unsichtbaren Fenster; unabhängig von Dauer und Shader der Animation, Daniels `window-close` 200 ms mit eigenem
    Shader). `screenshot window` bleibt wie es ist. Idee Daniels, statt des Vorschlags „warten, bis niri das Panel nicht
    mehr meldet, plus 200 ms“. **L38** bleibt (Bereichs-Screenshot zählt beim Öffnen der Auswahl). **L31**: keine Linie
    zwischen den Zeilen der Palette. **Nachtrag D3 (Daniel, 22.11.):** Fensterregel in `niri/dictate-binds.kdl` übernehmen
    (Titel „dictate: leaving“: `opacity 0.0`, Fokusring, Rahmen und Schatten aus), weil GTK-Deckkraft allein bei
    `prefer-no-csd` eine Fläche in Ringfarbe samt Schatten im Bild ließ; die GTK-Deckkraft bleibt als zweite Sicherung.
18. **Sicherheitsdurchsicht W5 (Block E, Daniel, 22.11.):** **M1** `text_action` aus der Palette zeigt die Vorschau der
    Markierung, die vorgewählte Aktion ist hervorgehoben und wird erst auf Return, Taste oder Klick gesendet (wie
    `explain_clipboard`; vorher startete sie wie unter Awesome sofort). **N3** eine offene Bestätigung (`confirm`) nimmt
    nur Return oder den Knopf „Confirm“, ein zweiter Klick auf dieselbe Zeile führt nicht aus. **H8** der Verlauf der
    Palette (50 Eingaben im Speicher) überlebt die Sperre (bleibt so).
19. **Offene Fragen nach W5 (Daniel, 22.11.):** (1) Die gesprochene Eingabe der Palette läuft weiter durch die ganze
    Pipeline des Diktats samt KI-Modus (bleibt so; die Diktat-KI ist in Daniels Config aus). (2) H2 bleibt: kein eigenes
    `Debug` mit Schwärzung, die Regel in CLAUDE.md genügt. (3) Die Logs der Textausgabe (`app_id` auf debug, Fenster-IDs
    beim Fokuswechsel) bleiben. (4) **L39 bestätigt, nach einem Versuch:** Esc im Abgang vor dem Ausführen wurde gebaut
    (Branch `w5-esc-leave`, verworfen) und wirkte im verschachtelten niri nicht: Esc 50 ms nach Return, trotzdem nahm die
    Frist nach 300 ms auf. Grund (abgeleitet): GTK 4 stellt Tasten erst mit dem nächsten Takt der Frame-Uhr zu, und die
    steht, bis niri den durchsichtigen Frame bestätigt (330–770 ms). Das Fenster für Esc wäre ohnehin höchstens 300 ms
    lang, auf dem echten Desktop vermutlich kürzer, also unter der Reaktionszeit. (5) Deckkraft 1/255 statt 0 bestätigt
    (Nachtrag zu Entscheidung 17). (6) Worktrees `wayland-w4` und `w7b-gui-niri` entfernt (Branches bleiben),
    `wayland-w5` bleibt.
20. **W6, Vorlagen V1–V5 (Daniel, 22.11.):** alle wie empfohlen (6h). **V1** Die Häufigkeit der Befehlspalette zieht
    nach `~/.local/state/dictate/frecency.json`, die Farbdatei bleibt `palette.json` (Regel `<tool>/palette.json` wie
    doorcam und niri-coach); Übernahme nur nach Inhalt, eine Farbdatei wird nie verschoben; behoben als Block A von W6,
    installiert mit W6. **V2** Das Einstellungsfenster folgt live dem `mode` der Palette (hell/dunkel). **V3** Das
    Pixelzeichen bleibt und wird in egui aus Rollen gezeichnet (`on_surface`, Akzentzelle `primary`), Wortmarke als Text;
    das Programm-Icon bleibt unverändert. **V4** About behält Name, „Pushing Pixels, Vienna“ und Link, das Zeichen (Bild)
    entfällt. **V5** `DESIGN.md` wird für die ganze Anwendung unter Noctalia neu gefasst; `design/pushingpixels/`,
    `assets/brand/` und nicht mehr gebrauchte PNGs werden gelöscht (bleiben in der Git-Geschichte). Die Lesarten
    L40–L47 gelten bis zum Widerspruch; Daniel noch nicht einzeln vorgelegt (sichtbar vor allem L40: keine Versalien
    und keine Laufweite mehr, und L42: Fensterregel für die Ecken in der KDL, über chezmoi).
21. **W6, offene Fragen aus Block A–C (Daniel, 22.11., einzeln abgefragt mit Empfehlung):** (1) **Kontrast bei hellen
    Schemata: die Palette gilt, wie sie ist** (Option b, nicht die Empfehlung a): Farben exakt wie Noctalia sie
    liefert, kein Nachschärfen, auch nicht in `dictate-ui`; Regel 5 (4,5:1) gilt nur für dunkle Schemata, helle werden
    nur gemessen (M5). (2) **Lesart L49 bleibt:** `primary` und `error` als Text auf `surface` wie im Panel. (3) **Clear
    der Häufigkeit wie empfohlen:** `pal_frecency_clear` entfernt zusätzlich eine `palette.json`, die
    `is_frecency_doc` erfüllt (strenge Erkennung, die Farbdatei wird nie angefasst, Vorsicht bei Inode und Deskriptor
    wie in `adopt_legacy`). (4) **Schrift:** Daniel wählte „Roboto installieren“; beim Prüfen zeigte sich, dass der
    Befund von M1 ein Messfehler war: Roboto ist seit 18.03.2024 installiert (`ttf-roboto` 3.016-1). M1 hatte
    `fc-match 'Roboto 12'` gefragt, fontconfig liest das als Familie „Roboto 12“ und fällt auf Noto Sans zurück; GTK
    zerlegt `gtk-font-name` als Pango-Beschreibung (Familie Roboto, 12 pt) und zeigt Roboto. Nichts zu installieren;
    `gtk-font-name` wird wie bei Pango zerlegt, nur die Familie geht an `fc-match`; die Schrift-Fixture ist `Roboto 12`.
22. **Abschluss W6 (Daniel, 22.11., einzeln abgefragt mit Empfehlung, alle wie empfohlen):** (1) installieren: alle drei
    Binaries, beide Dienste neu, KDL über chezmoi zusammen; (2) `wayland-w6` nach `main` mergen; (3) `main` und
    `wayland-w6` pushen; (4) `fc-match` bekommt eine Frist von 10 s (danach Prozesse beenden und einsammeln, Messpunkt
    M4 nachmessen; eigener Branch); (5) der Hauptknopf im Panel von `dictate-ui` setzt Text in `on_primary` statt
    `surface` (wie das Einstellungsfenster, Material 3).
23. **Live-Abnahme W2–W6 (Daniel, 23.11.):** Nach Benutzung am echten Desktop („ansonsten fühlt sich alles korrekt
    an“) gilt Phase 6 als live abgenommen, mit Vermerk, was nicht geprüft werden konnte (Liste „nicht belegt,
    beobachten“ in 6h, „Live-Abnahme W2–W6“). Einziger Befund: `Mod+Alt+Space` kollidierte mit der Suche von espanso
    (liest evdev vor niri); Daniel schaltete die Suche ab (`search_shortcut: off` in
    `~/.config/espanso/config/default.yml`), dictate behält die Taste. Danach: Aufräumen (Worktree `wayland-w5`
    entfernt, Build-Cache in den Worktree `phase6-abschluss` umgezogen, gemergte Branches lokal und auf `origin`
    gelöscht, chezmoi gepusht; alles mit Daniels Zustimmung; nach dem Umzug blieben `whisper-rs-sys` und `ring`
    frisch, `cargo build --release` übersetzte nur `dictate` neu, 2 min 48 s, ohne Warnung), als Nächstes Lizenz und
    Monetarisierung.

12. **Copy der Textaktionen (W3, 20.11.):** ohne Hinweis `x-kde-passwordManagerHint` (im Verlauf sichtbar wie unter X).
13. **Panelgröße (W3, 20.11.):** das Panel wächst mit dem Inhalt über niri-IPC (Resize-Shader wirkt), statt fest mit
    Scrollbereich.

10. **Zwischenablage mit Passwort (W2, 20.11.):** Trägt die Zwischenablage beim Einfügen den Hinweis
    `x-kde-passwordManagerHint: secret`, fasst dictate sie nicht an (gelesen werden nur die Typenliste und der Wert des
    Hinweises) und tippt stattdessen (T3, sonst T2). Das Passwort kommt nie in den Speicher von `dictate-ui`, der
    Passwortmanager behält Besitz und Ablauf. (Statt des Vorschlags aus S3, 30 s weiter zu bedienen.)
11. **Reihenfolge der Ausgabe (W2, 20.11.):** T3 zuerst: aktiviert das Zielfeld die Eingabemethode, wird der Text
    übergeben (Zwischenablage unberührt). Sonst außerhalb von Terminals Einfügen über Data-Control, in Terminals und im
    Zweifel T2. Zeilenumbrüche außerhalb von Terminals als Shift+Return über T2, in Terminals als Return.

14. **Abschluss W3 (Daniel, 20.11. abends):** Hinweis im Panel, wenn die Sperr-Erkennung aus ist (umgesetzt);
    Sperre bricht eine laufende Ausgabe samt Diktat ab und verweigert Mod+I (bleibt so); Tippen stoppt, sobald das
    Panel aufgeht, der Rest ist verloren (bleibt so); Lage-, Hover- und Klick-Zeilen im Debug-Log bleiben; W3 wird
    jetzt installiert, die niri-KDL übernimmt Claude über chezmoi; `wayland-w1..w3` jetzt nach `main` mergen und
    `main` sowie `wayland-w3` pushen; Auftrag W4 jetzt schreiben (6e). Daniel war um 22:15 gesperrt (der gesehene
    `LockedHint=yes` der Sitzung 2 ist damit erklärt).
15. **AwesomeWM entfernen (Daniel, 20.11. abends):** „Alles was AwesomeWM ist, kann weg.“ Vorgezogen vor die
    Live-Abnahme als eigener Block (W7, 6f) direkt nach dem Abschluss von W3, in frischer Sitzung; vorher eine
    Aufstellung zur Freigabe (Umfang: nur Awesome oder auch die X11-Wege). Folge: Palette und Diagnose-Panel sind bis
    W4/W5 nicht bedienbar (die Daemon-Teile bleiben).

Offen nach W0 (Daniel):

- ~~Zurückgelegte Passwörter (S3)~~: entschieden in W2 (Punkt 10).
- **Noctalia nach Updates neu starten** (kein dictate-Thema, aber am 20.11. die Ursache für das gescheiterte Entsperren):
  ggf. topgrade so einstellen, dass es nach einem Noctalia-Update daran erinnert.
