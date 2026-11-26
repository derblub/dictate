# Grundplan: den Desktop mit KI-Funktionen modernisieren

> **Seit W7 (21.11.2026):** dictate läuft unter niri (Wayland); AwesomeWM, die X11-Wege und die CLIs sind entfernt
> (`PLAN-wayland.md` 6f). Die Desktop-Schicht ist `dictate-ui` (GTK 4, Layer-Shell) statt Lua; was unten über Lua und
> Awesome steht, beschreibt den Weg bis dahin.

Ausgangspunkt war am 22.10.2026 die ChatGPT-Recherche „AwesomeWM KI Recherche“ (geteilter Chat, Inhalt hier
zusammengefasst) plus die Abgleiche mit dem echten System. Das Diktat (dieses Repo, v1 und v2) ist der erste Baustein.
Dieses Dokument hält fest, was darüber hinaus geplant ist.

## Leitidee

AwesomeWM nicht ersetzen, sondern die Lua-Konfiguration zu einer tastaturgesteuerten KI-Oberfläche erweitern.
KI dort, wo man sie braucht, kein dauernd sichtbarer Chatbot. **Rust-Kern, Lua als dünne Desktop-Schicht**: Rust macht
Logik, Modelle, Kontext und Berechtigungen; Lua macht Tasten, Tags, Fenster, Overlays. Lua blockiert nie (nur
`awful.spawn.easy_async`/`with_line_callback`, kein Polling), sonst friert der Desktop ein (zweimal passiert).

## Bausteine (Priorität von oben)

| # | Baustein | Stand | Notizen |
|---|---|---|---|
| 1 | **Diktat mit Live-Text** | fertig (v1, v2) | dieses Repo |
| 2 | **Textaktionen auf Markierung** | unter niri neu gebaut (W3, `dictate-ui`), installiert, live abgenommen am 23.11.2026 (`PLAN-actions.md`, `PLAN-wayland.md` 6d) | `Mod+I` → Panel (bis W7 in Awesome, kein Rofi), Antworten gestreamt, Copy oder Replace (nie in Terminals), Seite „Actions“ im Einstellungsfenster, standardmäßig aus. Ursprünglicher Plan: markierten Text erklären, übersetzen, umformulieren, Fehlermeldung analysieren, Code erklären. Kürzel → Rofi- oder Overlay-Menü → Ergebnis in Vorschau → kopieren oder ersetzen. Nutzt die vorhandenen KI-Presets und Modi aus `src/pipeline/ai.rs`. **Nur die PRIMARY-Markierung, nie automatisch die Zwischenablage** (Vaultwarden-Passwörter, clipmenud). Vor Cloud-Anfragen Vorschau, Filter für Inhalte, die wie Geheimnisse aussehen. |
| 3 | **Befehlspalette mit freier Sprache** | unter Awesome live abgenommen am 05.11.2026 (`PLAN-palette.md`); unter niri neu gebaut (W5, 22.11.2026: Panel in `dictate-ui`, Ausführen über die niri-IPC und per argv, `PLAN-wayland.md` 6g), am 22.11. installiert, unter niri live abgenommen am 23.11.2026 (Taste mit espanso entflochten, `PLAN-wayland.md` 6h) | `Mod+Alt+Space` (bis W7 `Mod+Space` in Awesome) → Panel von `dictate-ui`: Häufiges per festen Regeln ohne Modell (Fenster, Arbeitsflächen, Programme, Projekte, System), KI nur für freie Sprache und nur auf Verlangen (Enter ohne sicheren Treffer, `Ctrl+Enter`). Das Modell wählt aus einem **festen Katalog von Absichten** mit validierten Parametern; Suchbegriffe löst der Rechner lokal auf, ausgeführt wird nur über die niri-IPC mit fester ID bzw. per argv (bis W7 in Lua). Stimme per `Mod+E` im Panel, Text und KI über das Panel der Textaktionen (`ask`, Zwischenablage erklären nur mit Vorschau), Seite „Palette“ im Einstellungsfenster, bis W7 das schlanke Client-Binary `dictate-palette`. |
| 4 | **Linux-Diagnose** | unter niri fertig und installiert (W4, 21.11.2026: Panel in `dictate-ui`, Feld „What is wrong?“, Boot-Hinweis im Overlay; `PLAN-wayland.md` 6e), live abgenommen am 23.11.2026 (Hinweis nach echtem unsauberem Boot-Ende noch nicht gesehen; `PLAN-diagnose.md` als Archiv, Messung der Modelle `diagnose-ai.md`) | `Mod+Shift+I` → Diagnose-Panel von `dictate-ui` (dasselbe Fenster wie die Textaktionen, ein Panel zur Zeit): ein Profil wählen (Last crash, Now, Unit, Docker, Network, Heat), der Daemon sammelt **lesende** Proben aus einem festen Katalog von 33 Einträgen (bis W7 34, mit `xorg_log`) (journalctl, systemctl, Docker, Netz, Ressourcen, Hitze; nacheinander, gedrosselt, nie `sudo`, keine freien Befehle), die Vorschau zeigt genau, was gesendet wird (Geheimnisse geschwärzt statt gesperrt, Benutzer, Rechner, Adressen und Pfade pseudonymisiert), die KI liefert Befunde mit Belegen und Prüfbefehle **aus demselben Katalog**, die erst nach Bestätigung laufen (höchstens 5, bis zu drei Runden). Ändernde Aktionen gibt es nicht, Vorschläge dazu stehen nur als Text. Dazu die Palette-Absicht `diagnose` (unter niri seit W5), ein Hinweis nach unsauberem Boot-Ende (einmal je Boot, Klick öffnet die Diagnose), das Feld „What is wrong?“ (Symptom als gekennzeichnete Daten) und die Seite „Diagnose“ im Einstellungsfenster; standardmäßig aus. Passt zu den Freeze-Untersuchungen auf blub (`/home/blub/crash_debugging`). |
| 5 | **Kontext-Workspaces** | offen | Projektprofile: Tag, Programme, Terminal im Projektordner, passende Prompt-Vorlage. Fensterverwaltung bleibt deterministisch in Lua. |
| 6 | **Wayland unter niri** | fertig (`PLAN-wayland.md`): W0–W7 (AwesomeWM und X11 entfernt, Palette, Diagnose, Einstellungsfenster in Noctalia-Farben, `DESIGN.md` neu) gemergt, gepusht und installiert; W2–W6 live abgenommen am 23.11.2026 (Entscheidung 23; nicht belegt und zu beobachten: Liste in `PLAN-wayland.md` 6h) | blub ist auf niri gewechselt (Entscheidung 15: „Alles was AwesomeWM ist, kann weg“). `dictate-ui` übernimmt Overlay, Halten zum Sprechen, Textausgabe und Panels. |
| 7 | **Kleine Funktionen** | geplant (`PLAN-features.md` Abschnitt 2), nichts gebaut | Letztes Diktat erneut einfügen (Kürzel), Medien pausieren während des Diktats (MPRIS), durchsuchbarer lokaler Verlauf mit kurzer Aufbewahrung (Vorgabe aus), `dictate transcribe` für beliebige Audiodateien. Dazu das Ergebnis der Prüfung des GlobalShortcuts-Portals (Abschnitt 1: auf blub mit niri 26.04 nicht nutzbar, es fehlen der Provider `org.gnome.Settings.GlobalShortcutsProvider` (gemessen) und niris Tastenbelegung (#4744, gemergt, in keinem Release); Wiedervorlage K1b bei einem niri-Release; die Gruppe `input` bleibt). |
| 8 | **Freie Modellwahl über mehrere Engines, darin der CPU-Weg** | geplant (`PLAN-features.md` Abschnitt 3), nichts gebaut; Messung vom 26.11.2026 liegt vor | Engine-Abstraktion im Daemon (Whisper wie heute, ONNX Runtime mit Parakeet v3 int8 für Rechner ohne NVIDIA-Karte), Wahl je Profil, Modellkatalog mit Metadaten und „eigene Modelldatei“, Rückfall auf die CPU, `dictate bench <Modell>` auf den eigenen Testsätzen. Whisper bleibt die Vorgabe für Deutsch, bis eine Messung etwas anderes zeigt. |

## Sicherheitsregeln (für alle Bausteine)

- KI steuert nie direkt die Shell. Keine Modellantwort geht an `sh -c`. Nur Aktionen aus einer Allowlist.
- Ändernde Aktionen (Dateien, Pakete, Neustarts, Deployments) nur nach ausdrücklicher Bestätigung im Overlay.
- Keine Geheimnisse in Prompts (`secrets.env`, `.zshrc.secrets`, Schlüsseldateien, Passwörter aus der Zwischenablage).
- Datensparsam: nur den nötigen Text senden, nichts dauerhaft protokollieren (Verlauf nur auf Wunsch).
- Lokal bedeutet nicht automatisch sicher: auch lokale Modelle bekommen nur die enge Schnittstelle.

## Technik

- Vorhanden und wiederverwendbar: Daemon mit Socket-Protokoll, KI-Anbieter-Presets (Anthropic, OpenAI-kompatibel,
  Ollama, LM Studio), `dictate-ui` (Overlay, Panels, Textausgabe unter niri), Einstellungsfenster (egui) mit
  Feature-Schaltern, Design nach der Noctalia-Palette für die ganze Anwendung (`DESIGN.md`, seit W6).
- Für 2, 3 und 4 entschieden: der Daemon beantwortet `act_*`, `pal_*` und `diag_*` über den Socket; die Oberfläche
  (`dictate-ui`) spricht ihn direkt. Die CLIs `dictate act|palette|diag` und die Binaries `dictate-palette`,
  `dictate-diag` entfielen mit W7. Offen für 5: ob es in dieses Binary kommt oder in ein eigenes Tool mit gemeinsamer
  Bibliothek. Der Name `aura` aus der Recherche ist vergeben (Arch-AUR-Helper).
- Lokale Modelle: RTX 3060 mit 12 GB, 62 GB RAM; Whisper belegt 1,2–2,2 GB, daneben passt ein 7B-Modell (q4).
  Ollama ist installiert, der Dienst aus.
