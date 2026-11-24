# dictate v2 – Einstellungsfenster, Feature-Schalter, Post-Processing (Spezifikation)

> **Archiv (W7, 21.11.2026):** Beschreibt den Stand unter AwesomeWM/X11; AwesomeWM, die X11-Wege und die CLIs sind
> entfernt, dictate läuft unter niri (Wayland). Gültig sind `README.md` und `PLAN-wayland.md`; der Inhalt hier bleibt
> unverändert.

Stand 22.10.2026. Lead: Hauptsitzung. Baut auf v1 auf (`PLAN.md`, `README.md`). Dieses Dokument ist der Vertrag zwischen
den Aufgaben; Schnittstellen hier ändern nur nach Absprache mit dem Lead.

## Entscheidungen des Users

- **Plattform:** Linux allgemein. Daemon und Einstellungsfenster bleiben desktop-unabhängig; das Awesome-Modul bleibt
  das Frontend für blub. Ein eigenständiges Overlay für GNOME/KDE/Wayland ist **Phase 6**, nicht Teil dieser Runde.
- **Kein Premium mit Sperren:** Alle Features sind frei und einzeln schaltbar. Es gibt eine „Support“-Seite mit
  Spendenlink (URL vorerst Platzhalter in der Config, `support_url`). Keine Lizenzlogik.
- **Feature-Reihenfolge:** Sprachbefehle → Verlauf + Vokabular → Profile pro App → KI-Nachbearbeitung.
- **GUI:** egui über `eframe` (Lead-Wahl aus „egui/iced“: ausgereifter für Formulare).
- **Design:** Pushing Pixels Design Guide, übersetzt in `DESIGN.md` (gilt für Overlay, Fenster und alle Texte; nur Englisch).

## Architektur

```
dictate-settings (eframe)  ──control.sock──▶  dictate daemon  ──▶ Streamer ──▶ Pipeline ──▶ final
        │ (ohne Daemon: liest/schreibt TOML direkt)        │                     │
        └──────────── ~/.config/dictate/config.toml ◀──────┘ reload              ├─ commands  (Sprachbefehle)
                                                                                 ├─ vocabulary (Ersetzungen)
Awesome-Modul ◀── listen --format awesome (inkl. overlay-Konfig) ───────────────├─ profile    (Regeln je App)
   start --app <WM_CLASS>                                                        └─ ai         (Ollama / OpenAI-kompatibel)
```

- Zweites Binary `dictate-settings` im selben Crate (`src/bin/dictate-settings.rs` + Modul `src/gui/`), hinter
  Cargo-Feature `gui` (Default an). Der Daemon-Code zieht keine GUI-Abhängigkeiten.
- Die TOML-Datei ist die einzige Quelle der Wahrheit. Schreiben immer über `toml_edit` (Kommentare des Users bleiben
  erhalten), atomar (temp + rename), Modus 0600.

## Konfiguration (Erweiterung von `src/config.rs`)

Bestehende Schlüssel bleiben. Neu (alle optional, `deny_unknown_fields` bleibt):

```toml
support_url = ""                 # Spendenlink für die Support-Seite

[features]                       # Hauptschalter je Feature
voice_commands = true
vocabulary = true
history = false                  # Datenschutz: aus, bis der User es einschaltet
profiles = true
ai = false
actions = false                  # Textaktionen auf der Markierung (Mod+i), Tabelle [actions]: README.md, PLAN-actions.md

[overlay]                        # wird an das Awesome-Modul gestreamt, Lua-Defaults gelten sonst
position = "bottom"              # bottom | top
max_width = 720
transparency = "auto"            # auto | argb | window | none
opacity = 0.8
bottom_margin = 48

[typing]
delay_ms = 12
trailing_space = true            # ersetzt den alten Top-Level-Schlüssel; alter Schlüssel bleibt als Alias lesbar

[history]
max_entries = 200                # Datei ~/.local/state/dictate/history.jsonl (0600)

[vocabulary]
terms = ["PostgreSQL", "AwesomeWM", "chezmoi"]     # gehen als Hinweis in den Whisper-Prompt
replacements = { "post gress" = "PostgreSQL" }     # nach der Erkennung, ganze Wörter, Groß/klein-unabhängig

[[profiles]]                     # erste passende gewinnt; ohne Treffer gilt das Profil "default"
name = "terminal"
match_class = ["Alacritty", "kitty", "XTerm"]      # WM_CLASS des fokussierten Clients (exakt, Groß/klein egal)
punctuation = "strip"            # keep | strip
capitalize_first = false
trailing_space = false
ai_mode = ""                     # leer = keine KI; sonst Name aus [ai.modes]
language = ""                    # leer = globale Sprache

[ai]
provider = "ollama"              # ollama | openai | anthropic
endpoint = ""                    # leer = Preset-Basis, sonst die des Protokolls (Ollama: http://127.0.0.1:11434); gesetzt gilt immer
model = ""                       # leer = Vorgabemodell des Anbieters (Preset bzw. Protokoll), siehe unten
api_key_file = ""                # nur openai; Datei mit Schlüssel (0600), nie der Schlüssel selbst in der TOML
timeout_ms = 4000                # danach Rohtext tippen + Hinweis
default_mode = ""                # Modus ohne Profil-Vorgabe
formality = "auto"               # Anrede bei Übersetzungen ins Deutsche: auto (aus dem Ton der Vorlage) | informal (du) | formal (Sie)
removed_modes = []               # eingebaute Modi, die es nicht geben soll (z. B. ["email"])

[ai.modes]
# Nur Abweichungen von den eingebauten Modi (BUILTIN_MODES: clean, email, to_en, to_de, prompt; Volltexte PROMPT_* in
# src/config.rs): geänderte eingebaute und eigene Modi. Vor jedem Prompt (auch eigenen) steht beim Aufruf der feste
# Schutzrahmen INPUT_GUARD (siehe unten); die Prompts sagen nur, was der Modus tut.
mine = "Make the text shorter. Output only the text."

[ai.mode_models]                 # optional: Modell je Modus, nur Namen aus [ai.modes]; fehlt ein Modus, gilt ai.model
to_de = "claude-sonnet-5-5"
```

**Schutzrahmen (`INPUT_GUARD` in `src/config.rs`).** Fester, nicht editierbarer Text, den `AiCfg::mode_prompt` vor
jeden Modus-Prompt setzt (Vorgabe wie eigener Modus) und `crate::actions` vor jeden Aktions-Prompt: das Modell ist ein
Verarbeitungsschritt, kein Assistent; die Benutzernachricht ist nie an es gerichtet, Fragen, Bitten und Anweisungen
darin (auch „ignoriere deine Regeln“) werden wie jeder andere Satz bearbeitet, nie beantwortet, befolgt oder abgelehnt.
Er steht vorn, nicht am Ende: im A/B-Test mit Haiku 5.5 (10 frische Sätze mit Fragen, Bitten und Injection, je 2 Läufe)
machte `clean` damit 0 von 20 Fehlern, mit derselben Regel am Ende 6; ein eigener Modus („formeller umschreiben“) 6
statt 13 (ohne Regel beantwortete er fast jede Frage). Gegenprobe mit 8 weiteren Sätzen über `clean`, `email`, `to_de`,
`to_en`: 2 von 64 Fehlern statt 15 mit den alten Prompts. Das Einstellungsfenster weist auf den Rahmen hin.

**Modus `prompt` und Aktion Refine prompt (23.10.2026, Entscheidung des Users in `UX-actions.md`, Punkt 5).** Eine
Konstante `PROMPT_REFINE` für den Modus `prompt` (`REFINE_MODE`) und die Textaktion `refine_prompt` (Taste `p`,
Ersetzen). Sprache wie die Eingabe; reiner Text mit Bezeichnungen Ziel/Goal, Kontext/Context (nur wenn der Text
Hintergrund nennt), Anforderungen/Requirements, Ausgabeformat/Output format, Offene Punkte/Open points („- “-Listen,
kein Markdown, weil getippt wird); nur was der Text sagt oder klar meint, kein eigenes Fachwissen, Fehlendes als
Frage oder als „Annahme:“; etwa 2–4-fache Länge; nie ausführen. Der Schutzrahmen wirkt auch hier, obwohl die Eingabe
selbst eine Anweisung ist: im A/B mit Haiku 5.5 (8 Eingaben, darunter t16 und zwei Fragen, zwei Varianten, plus zwei
längere) wurde keine von 18 ausgeführt oder beantwortet, Sprache und Gliederung stimmten immer. Variante B (kein
Fachwissen, Kontext sonst weglassen) senkte Zusätze von 3 auf 1 von 8. Länge: Kurzeingaben (34–102 Zeichen) ergeben
650–860 Zeichen, längere (260–330) das 3,6- bis 5,5-Fache. Protokoll: `/home/data/stt-test/logs/refine-prompt-ab.md`.
Neu (Entscheidung des Users 23.10., zweite Fassung): Ziel zwei- bis dreifach, strikt höchstens das Dreifache, ohne
Untergrenze in Zeichen (`refine_target_chars` = 3× Eingabe); sehr kurze Eingaben nur Ziel und ein, zwei Anforderungen.
Live (Fassungen E/F, 12 Aufrufe): kurze Eingaben 3,9–5,7×, drei sehr kurze in E über 6× (Diktat: Rohtext), lange
2,5× und 3,6×; ≤ 3× nicht zuverlässig, Befund in `refine-prompt-ab.md` (Nachtrag 2), offen für eine Entscheidung.
Erste Fassung desselben Tages: max(3×, 400 Zeichen); nur
„Ziel“ ist Pflicht, übrige Abschnitte nur mit Inhalt. Live (Variante C): 7 von 10 im Ziel, Überschreitungen bis 25 %.
**Dritte Fassung (23.10., Nachtrag 3 im Protokoll): das Limit steht in der Anfrage.** Ein Verhältnis im festen Prompt hielt
Haiku bei kurzen Eingaben nicht ein (4–6×). Jetzt rechnet der Code je Anfrage die Zeichenzahl und hängt sie als festen,
nicht editierbaren Zusatz ans Ende des System-Prompts (`config::length_budget_note`/`with_length_budget`; Diktat:
`pipeline::ai::rewrite_impl` über `Shape::budget`, Aktion: `Action::prompt_for` für `grows`). Der Zusatz enthält nur
Zahlen aus der Länge (kein Text der Eingabe, also keine Einfallstür für Injection), steht hinter Schutzrahmen und Prompt
und gilt auch für einen selbst geänderten Prompt des Modus. Geprüft wird gegen 3× die Eingabe, genannt werden 80 %
davon (`BUDGET_STATED_PERCENT`; Modelle überschreiten eine genannte Zahl). `PROMPT_REFINE` enthält kein Verhältnis mehr,
nur die Staffelung nach dem Limit (unter 100 Zeichen nur Ziel, unter 150 dazu höchstens eine Zeile Anforderungen, unter
300 ein, zwei Zeilen). Live (Haiku 5.5, 12 Aufrufe): 9 von 10 Eingaben ≤ 3× (vorher 1 von 10), kurze 1,1–1,2×, t16 3,2×;
eine falsch übersetzte Bezeichnung („Requirements:“ in deutscher Antwort). Nachtrag 4 (10 Aufrufe) widerlegt die beiden
naheliegenden Schritte: 70 % genannt und die Bezeichnungen je Sprache ausdrücklich vorgeben (Abschnitte sonst klein
benannt) brachten t16 auf 3,8-fach (genannt 214, geschrieben 389), bash und Flask auf 3,05-fach und ließen vier von fünf
englischen Eingaben auf Deutsch antworten (Inhalt und Bezeichnungen); 7 von 10 ≤ 3×. Nicht übernommen, der Stand ist die
Fassung von Nachtrag 3 (80 %). Offen für eine Entscheidung: t16 (Aufgabenprompts erzeugen gern Anforderungen und offene
Punkte, auch gegen „nur Ziel und Anforderungen“), Bezeichnungen anders sichern (z. B. nach der Antwort prüfen und bei
falscher Sprache den Rohtext nehmen) statt im Prompt. Mechanismus allgemein: `Shape::budget` je Modus; andere
Modi (`clean`, `email`, `to_*`, eigene) bekommen weder Zusatz noch eigene Grenzen.
Harte Grenze `refine_max_chars` = 150 % des Ziels (4,5× die Eingabe; gemessen höchstens 1,27× das Ziel, t16 in Nachtrag 4, vorher
200 %): darunter wird beim Diktat getippt und im Panel mit Hinweis (`note`) gezeigt, darüber gilt die Antwort als
ausgeführt (Diktat: Rohtext; Aktion: `max_tokens`, abgeschnitten, nie getippt).
Pipeline (`pipeline::ai::Shape`, nach dem Modusnamen): nur `email` und `prompt` behalten neue Zeilen; `prompt` hat
statt der Dreifach-Grenze `refine_max_chars` (4,5×, ohne Untergrenze), passende `max_tokens`
(`actions::replace_tokens`, 2 Zeichen je Token) und mindestens 20 s Zeitgrenze (Haiku brauchte 1,7–4,2 s, die
Vorgabe 4 s für kurze Korrekturen hätte einmal gegriffen). Die Aktion rechnet `max_tokens` ebenso (`Builtin::grows`).
Im Aktionsmenü fehlt `mode:prompt`, solange der Modus wortgleich die Vorgabe ist (`actions::covered_mode`): so steht
nichts doppelt und `to_de`/`to_en` behalten die Tasten 3/4. Zeilenumbrüche tippt das Awesome-Modul als Shift+Return
(Chat-Felder senden nicht vorzeitig), in Terminals (`actions.terminal_classes`, Event `terminals`) als Return.

**Anrede (`ai.formality`).** `auto`: `to_de` leitet du oder Sie aus dem Ton der Vorlage ab (locker, Vornamen, „hey“ → du;
förmlich, geschäftlich → Sie) und bleibt innerhalb eines Texts bei einer Form. `informal`/`formal` hängt an den
`to_de`-Prompt (auch an einen eigenen) eine Zeile, die du bzw. Sie für den ganzen Text festlegt; dieselbe Zeile bekommt
die Textaktion `translate` für die Richtung ins Deutsche. Andere Modi bleiben unberührt.

**Modi und Aktions-Prompts in der Datei (seit 23.10.2026).** Im Speicher (`AiCfg::modes`, `ActionsCfg::prompts`) und
im JSON von `get_config`/`set_config` stehen alle wirksamen Modi bzw. alle Aktions-Prompts; in `ai.modes` des JSON
fehlt ein entfernter Modus. Die Datei speichert nur Abweichungen von den Vorgaben: `Config::from_toml` legt `[ai.modes]`
über `BUILTIN_MODES` und nimmt `ai.removed_modes` heraus (nur eingebaute Namen, nicht zugleich in `[ai.modes]`, sonst
Feldfehler `ai.removed_modes[i]`), `Config::render_toml` schreibt nur Einträge, die nicht wortgleich der Vorgabe sind,
und die entfernten eingebauten Modi. `[actions.prompts]` ebenso, ohne Entfernen (Aktionen sind fest). So erreichen
verbesserte Vorgabe-Prompts jeden, der den Modus nicht geändert hat. Grund: das Fenster hatte beim ersten Apply mit
neuer `[ai]`-Tabelle alle Modi wortgleich geschrieben, früher ersetzte eine vorhandene Tabelle die Vorgaben ganz, und
neue Prompts kamen nie an. Rückwärts: solche Einträge lesen sich gleich und entfallen beim nächsten Speichern; eine
ältere Datei, die eingebaute Modi bewusst wegließ, bekommt sie zurück (bisher nur dieser eine User, dessen Datei alle
enthielt). Leere Map-Tabellen (`[ai.mode_models]`, `[vocabulary.replacements]`) entfallen samt Kopfzeile.

**Vorgabemodell (`ai.model` leer).** Leer heißt Vorgabe des Anbieters (`AiCfg::effective_model`): das Feld
`default_model` des Presets in `AI_PRESETS` (Anthropic `claude-haiku-5-5`, Ollama `qwen2.5:7b-instruct`), ohne Preset
bzw. bei Custom die des Protokolls (anthropic, ollama; openai keins). OpenAI-kompatible Presets haben keins: der Katalog
hängt vom Server ab (LM Studio, OpenRouter) oder ist hier nicht geprüft. Früher stand fest `qwen2.5:7b-instruct` im
Default; nach dem Wechsel zu Anthropic blieb es stehen und die API lehnte ab (Live-Test 23.10.2026). Prüfung
`AiCfg::check_model`: Fehler, wenn die Anthropic-API (`api.anthropic.com`) ein Modell ohne `claude-` bekommt oder ein
anderes Cloud-Preset ein `claude-…`; Warnung bei eigenen Servern (Anthropic-Protokoll ohne „claude“ im Namen, lokale
bzw. Custom-Server mit `claude-…`). `Config::validate_new` prüft das beim Speichern (`set_config`, Fenster ohne Daemon),
nur wenn KI oder Textaktionen an sind, samt Pflichtmodell ohne Vorgabe; beim Laden nicht (eine ältere Datei soll das
Diktat nicht blockieren), dann meldet `pipeline::ai::chat` denselben Grund ohne Anfrage. Das Fenster
(`Settings::choose_ai_preset`): ein anderes Preset leert Endpoint (Preset-Basis, als Platzhalter sichtbar),
Schlüsseldatei und Modelle (`ai.model`, `ai.mode_models`, `actions.model`), außer Claude-Modellen bei einem
Anthropic-Server (`AiCfg::serves_model`); zurück zum gespeicherten Preset stellt dessen Felder her, Custom behält den
Server.

**Modell je Modus (`ai.mode_models`).** Diktat mit Modus `m`: `ai.mode_models.m`, sonst `ai.model` (leer: Vorgabemodell). Textaktionen: bei
`mode:<name>` zuerst `ai.mode_models.<name>`, dann `actions.model`, dann `ai.model`; eingebaute Aktionen
`actions.model`, dann `ai.model`. Das Speziellste gewinnt: ein Modusmodell gehört zum Prompt dieses Modus (etwa ein
stärkeres Modell für `to_de`), und derselbe Modus soll beim Diktat und auf der Markierung gleich arbeiten.
`actions.model` ist der allgemeine Standard für Aktionen. Validierung: unbekannter Modus oder leerer Wert ist ein Fehler.
Kein Vorbelegen von `to_de` mit Sonnet: Messung vom 22.10.2026 mit dem geschärften `to_de`-Prompt (22 frische Sätze,
Anreden, Relativsätze, du/Sie, je 3 Läufe) ergab Haiku 5.5 4 von 66 fehlerhaften Ausgaben, Sonnet 5.5 3 von 66 (beide
„Lieber Sam“ bei unbekanntem Geschlecht, Haiku einmal Sie statt du), Latenz Median 1,0 s gegen 1,5 s (p90 1,7 s gegen
3,2 s). Vor der Schärfung (Zeilenumbrüche nach der Anrede, erfundenes „Herr“/„Frau“ bei „Dear Dr. …“) waren es 11 gegen
4 von 40. Sonnet ist damit nicht klar besser; wer es will, setzt `to_de` hier selbst.

**Denken bei Claude (`provider = "anthropic"`).** Für die Nachbearbeitung soll das Modell nicht denken (Latenz, und
Denken zählt in `max_tokens`). Welcher Parameter gilt, hängt vom Modell ab (Claude-API-Doku, Tabelle „Thinking &
Effort“, Stand Oktober 2026); ein falscher ist HTTP 400:

| Modell | gesendet |
|---|---|
| Sonnet 5.5 | `thinking: {type: "between_tools"}` (`disabled` ist 400; ohne Werkzeuge kein Denken) |
| Opus 5, Sonnet 5, Haiku 5.5 | `thinking: {type: "disabled"}` (erlaubt bis effort `high`; wir senden kein effort) |
| Opus 5.5, Fable, Mythos | kein `thinking` (nicht abschaltbar), `output_config.effort = "low"`, `max_tokens` + 8192 |
| Claude 3.x, Opus/Sonnet/Haiku 4.x | nichts (ohne Parameter kein Denken) |
| unbekannt | `disabled`; HTTP 400 zu thinking/effort → nächste Stufe laut Meldung (`between_tools`, sonst effort low, sonst nichts), höchstens 3 Versuche, Ergebnis je Modell für den Daemon-Lauf gemerkt |

Nie `temperature` (aktuelle Modelle: 400). Plattform-Präfixe und Datumszusätze (`anthropic.…-v1:0`, `@2025…`,
`-20250929`, `-latest`) zählen zum Grundmodell. `stop_reason` `max_tokens` und `refusal` sind Fehler (Rohtext bleibt).

**Werkzeuge bei Claude** (nur strukturierte Antworten, KI-Pfad der Befehlspalette, `ToolCaps` in `src/pipeline/ai.rs`,
Claude-API-Doku „Tool use“ und „Structured outputs“, Stand Oktober 2026): `tool_choice` `any` (genau ein Aufruf
erzwungen) nur ohne Denken (`thinking` weggelassen oder `disabled`); Opus 5.5, Sonnet 5.5, Fable 5.1 und Mythos 5.1
lehnen erzwungene Wahl mit HTTP 400 ab, dort `auto` mit der Anweisung im Prompt, immer `disable_parallel_tool_use`.
`strict: true` je Werkzeug außer Claude 3 (strict kennt keine `minimum`/`maximum`/`minLength`/`maxLength` und kein
`oneOf`; die Grenzen prüft Rust). Unbekannte Modelle: HTTP 400 mit `tool_choice` → `auto`, mit `strict` → ohne strict,
Ergebnis je Modell für den Daemon-Lauf gemerkt (zusätzlich zu den Rückfällen beim Denken).

## Socket-Protokoll (Erweiterung)

Bestehende Befehle bleiben. Neu:

| Befehl | Antwort |
|---|---|
| `{"cmd":"start","app":"Alacritty"}` | wie bisher; `app` optional, wählt das Profil (Awesome sendet `client.focus.class`) |
| `{"cmd":"get_config"}` | `{"ok":true,"config":{…vollständige Config als JSON mit Defaults…},"path":"…","schema":2}` |
| `{"cmd":"set_config","config":{…}}` | validieren, atomar schreiben, neu laden; `{"ok":true,"reload":"live"\|"model"}` oder `{"ok":false,"error":"…","field":"ai.timeout_ms"}` |
| `{"cmd":"reload"}` | TOML neu lesen (nach Fremdänderung) |
| `{"cmd":"list_sources"}` | `{"ok":true,"sources":[{"name":"…","description":"…","default":true}]}` |
| `{"cmd":"list_models"}` | `{"ok":true,"installed":[{"file":"…","size":…}],"catalog":[{"id":"large-v3-turbo-q5_0","url":"…","sha256":"…","size":…,"note":"…"}]}` |
| `{"cmd":"history","limit":50}` | `{"ok":true,"entries":[{"ts":"…","app":"…","raw":"…","text":"…"}]}` (leer, wenn Feature aus) |
| `{"cmd":"history_clear"}` | löscht die Datei |
| `{"cmd":"ai_models"}` | Ollama `/api/tags` durchreichen; Fehlertext, wenn Ollama nicht läuft |
| `{"cmd":"meter","on":true}` | öffnet das Mikrofon nur für den Pegel (Audio-Seite im GUI), sendet `level`-Events, schließt bei `on:false`, beim Trennen der Verbindung oder spätestens nach 60 s |
| `act_grab`, `act_run`, `act_cancel`, `act_result`, `act_clear` | Textaktionen auf der Markierung (Baustein 2), Protokoll und `delta`-Zeilen in `PLAN-actions.md` |

Reload-Regeln: Modell-, VAD-, Sprach- und Thread-Änderungen → Modell im Hintergrund neu laden (`reload:"model"`,
Zustand `loading`); alles andere sofort (`live`). Während `listening` wird ein Reload bis zum Ende der Sitzung verschoben.

Events (neu): `{"event":"overlay","overlay":{…[overlay]-Tabelle…}}` an jeden Subscriber direkt nach dem State-Event
und nach jeder Änderung. `final` bekommt zusätzlich `"raw":"…"` (Text vor der Pipeline) und `"keys":[…]` (siehe
Sprachbefehle).

`--format awesome`: Textfelder werden ab v2 **escaped statt bereinigt**: `\\` → `\\\\`, Tab → `\t`, Zeilenumbruch → `\n`
(jeweils Backslash + Buchstabe). Lua dekodiert. Neue Zeilen:
```
overlay<TAB>position=bottom<TAB>max_width=720<TAB>transparency=auto<TAB>opacity=0.8<TAB>bottom_margin=48
final<TAB><text><TAB><keys>      keys = durch Kommas getrennte xdotool-Tastennamen, z. B. "BackSpace,BackSpace" (leer = keine)
```

## Pipeline (`src/pipeline/`, nach `Streamer::finish`, vor dem `final`-Event)

Reihenfolge, jeder Schritt nur bei eingeschaltetem Feature bzw. wenn das Profil ihn verlangt:

1. **Sprachbefehle** (`commands.rs`). Deutsch und Englisch, ganze Phrasen, groß/klein- und satzzeichen-unabhängig:
   „neue Zeile“/„new line“ → `\n`; „neuer Absatz“/„new paragraph“ → `\n\n`; „Komma“, „Punkt“, „Fragezeichen“,
   „Ausrufezeichen“, „Doppelpunkt“, „Bindestrich“ (+ englisch) → Zeichen, an das vorige Wort angehängt; „lösch das
   letzte Wort“/„delete last word“ → entfernt das letzte Wort im **aktuellen** Diktat; „alles löschen“/„scratch that“ →
   verwirft das aktuelle Diktat; „rückgängig“/„undo that“ als **einziges** Diktat → `keys` = BackSpace × Länge des
   zuletzt getippten Diktats (nur, wenn seitdem kein anderes Diktat getippt wurde; Daemon merkt sich Länge in
   Graphem-Näherung, App und Zeitpunkt). Wirkt nur in **derselben App** und **innerhalb von 30 s**; in einer anderen App
   oder später ist es ein gewöhnliches Wort, ohne vorheriges Diktat kommt kein Text und kein Tastendruck, nur der
   `error`-Event „Nothing to undo“. Höchstens 2000 BackSpace.
   Wörtliche Nutzung: „wörtlich Komma“ tippt das Wort. Befehlsliste als Tabelle im Code, im GUI sichtbar.
   Whisper schreibt Befehle oft schon als Zeichen („Komma“ → „,“) – beide Fälle abdecken, Tests mit echten
   Whisper-Ausgaben.
2. **Vokabular** (`vocabulary.rs`): `replacements` auf Wortgrenzen; `terms` fließen in den Whisper-Prompt
   (`initial_prompt` + Begriffe, gekürzt auf ~200 Zeichen) – das ist eine Änderung im Streamer, minimal halten.
3. **Profil** (`profile.rs`): Auswahl per `app`; `punctuation=strip` entfernt Satzzeichen außer innerhalb von Wörtern
   (z. B. „v1.2“), `capitalize_first`, `trailing_space`, `language` (Sprachwechsel pro Sitzung ohne Modell-Reload,
   whisper-Parameter), `ai_mode`.
4. **KI** (`ai.rs`): HTTP ohne Async-Runtime (eigener Client `src/http.rs`), Ollama `/api/chat` mit `stream:false`, bzw. OpenAI-kompatibel
   `/v1/chat/completions`. System-Prompt = Schutzrahmen + Modus-Text (bei `to_de` mit Anrede-Zeile), User = Text, Modell aus
   `ai.mode_models` bzw. `ai.model`. Antwort höchstens dreifach so lang wie die Eingabe und ohne neue Zeilen, außer
   bei `email` (Zeilen) und `prompt` (Zeilen, 5×, Zeitgrenze mindestens 20 s; `Shape` in `ai.rs`). Timeout `timeout_ms` → Rohtext + `error`-Event
   „AI timeout, raw text typed“. Während der KI-Anfrage bleibt der Zustand `finalizing`. Kein Logging von Text außer
   mit `DICTATE_DEBUG=1`.
5. **Verlauf** (`history.rs`): bei `features.history` jede nicht abgebrochene Sitzung als JSON-Zeile
   (`ts`, `app`, `raw`, `text`) an `history.jsonl` anhängen, auf `max_entries` kürzen.

## Einstellungsfenster `dictate-settings`

- Linke Seitenleiste, rechts Inhalt. Seiten: **General** (Sprache, Tippen, Kürzel-Übersicht nur lesend),
  **Audio** (Quelle als Dropdown aus `list_sources`, Live-Pegel über `meter`), **Model** (installierte Modelle,
  Katalog mit Download inkl. Fortschritt und SHA-256-Prüfung nach `~/.local/share/dictate/models/`, Idle-Entladen),
  **Recognition** (Pausenlänge, max. Aufnahmedauer, Blocklist), **Overlay** (alle `[overlay]`-Werte), **Voice commands**
  (Schalter + Befehlstabelle), **Vocabulary** (Begriffe, Ersetzungen als editierbare Tabelle), **Profiles** (Liste,
  Editor; „Use focused app“ ist nicht möglich – Klassenname eintippen, Hinweis wie man ihn findet), **AI** (Provider,
  Endpoint, Modell-Dropdown aus `ai_models`, Modi editierbar, „Test“-Knopf mit Beispieltext), **Actions**
  (Textaktionen, `[actions]`: Server-Hinweis, Modell, Frist, Länge, Geheimnisfilter, Antwortsprache, KI-Modi,
  Terminal-Klassen, Prompts der eingebauten Aktionen mit Zurücksetzen, Kürzel `Mod+i`), **History** (Liste,
  Kopieren, Löschen; nur wenn eingeschaltet), **Support** (Text + Link `support_url`, „Open“ per `xdg-open`).
- Speichern: „Apply“ schickt `set_config`; Feldfehler werden am Feld angezeigt. Ohne laufenden Daemon schreibt das GUI
  die TOML selbst (gleiche Validierung aus `config.rs`) und zeigt ein Banner „Daemon not running – changes apply on
  next start“.
- Einzelinstanz (zweiter Start bringt das vorhandene Fenster nach vorn oder beendet sich mit Hinweis).
- Desktop-Datei `dictate-settings.desktop` (Kategorie Settings) und Awesome-Kürzel `Mod+Ctrl+Shift+e` öffnet das
  Fenster (Lead bindet ein).
- Kein Polling des Daemons: Verbindung halten (`subscribe`) für State/Level, Befehle auf eigener Verbindung.

## Awesome-Modul (Änderungen)

- `start` sendet `--app <client.focus.class>` (CLI: `dictate start --app NAME`).
- `overlay`-Zeilen übernehmen (Laufzeit-Änderung von Position, Breite, Transparenz, Deckkraft ohne Awesome-Neustart).
- Escaping dekodieren; `final` mit Text und `keys`: erst Text tippen (Zeilenumbrüche als echte `\n` an `xdotool type`),
  dann `xdotool key --clearmodifiers <keys…>`; alles in der vorhandenen Tipp-Warteschlange.
- Kürzel `Mod+Ctrl+Shift+e` → `dictate-settings` (Argumentliste, ohne Shell).

## Phase 6 (später, nicht in dieser Runde)

Eigenständiges Overlay `dictate overlay` für andere Desktops (X11: undekoriertes Always-on-top-Fenster mit Maus-Durchgriff;
Wayland: wlr-layer-shell, GNOME ohne layer-shell nur über Extension), globale Kürzel über xdg-desktop-portal
GlobalShortcuts, Tippen über `wtype`/`ydotool`. Paketierung (AUR, Flatpak scheidet wegen Mikrofon/Tippen weitgehend aus).

## Aufgaben und Wellen

| Welle | Aufgabe | Modell | Dateien |
|---|---|---|---|
| 1 | **A Config + Protokoll** | Sonnet | `config.rs` (Schema, toml_edit-Schreiben, Validierung, Alias), `protocol.rs`, `daemon.rs` (neue Befehle, Reload, overlay-Event, `meter`, `app`), `client.rs` (Escaping, `--app`), `audio.rs` (`list_sources`), `src/models.rs` (Katalog, Downloads macht das GUI) |
| 1 | **B GUI** | Sonnet | `src/bin/dictate-settings.rs`, `src/gui/**`, `Cargo.toml` (Feature `gui`), `packaging/dictate-settings.desktop`. Gegen das Protokoll oben, Test mit Mock-Daemon (eigener Socket) und gegen den echten Daemon, sobald A fertig ist. Screenshots unter Xvfb. |
| 2 | **C Pipeline: Befehle, Vokabular, Profile, Verlauf** | Opus | `src/pipeline/{mod,commands,vocabulary,profile,history}.rs`, Anbindung in `daemon.rs` (minimal, nach `finish`), Prompt-Ergänzung im Streamer |
| 2 | **D Awesome-Modul v2** | Sonnet | `awesome/dictate/init.lua`, `awesome/test/**` |
| 3 | **E KI** | Sonnet | `src/pipeline/ai.rs`, GUI-Seite AI (Zusammenarbeit über klar abgegrenzte Datei `src/gui/pages/ai.rs`) |
| 3 | **Integration + Review** | Lead + Sonnet-Reviewer | Installation, rc.lua, Live-Check, README |

Regeln für alle: nur eigene Dateien stagen (`git add <pfad>`), nie `git add -A`; Commits lokal auf main, kein Push;
`cargo build --release`, `cargo test --release`, `cargo clippy --release --all-targets` ohne Warnungen; lange
Ausgaben nach `/home/data/stt-test/logs/v2-*`; Dienst `dictate.service` und die echte Awesome-Sitzung (DISPLAY=:0)
nicht anfassen; kein `cargo install`; Prozesse nur per PID beenden, nie `pkill -f`. Bericht ≤ 30 Zeilen.

## Abnahme

1. Alle Seiten des GUI funktionieren gegen den echten Daemon; Änderungen wirken ohne Neustart (Overlay-Breite live in
   Awesome sichtbar, Mikrofonwechsel bei nächster Aufnahme).
2. „Hallo Komma wie geht's Fragezeichen neuer Absatz Gruß“ ergibt `Hallo, wie geht's?\n\nGruß`.
3. Im Terminal-Profil keine Satzzeichen, kein großer Anfangsbuchstabe.
4. KI-Modus `clean` mit Ollama liefert bereinigten Text; bei gestopptem Ollama Rohtext + Hinweis nach `timeout_ms`.
5. Verlauf nur bei eingeschaltetem Feature, Datei 0600, Löschen funktioniert.


## Sicherheit der KI-Anbindung und Nachträge (Review v2, 22.10.2026)

- **Endpoint:** Vorgabe ist leer. Ein ausdrücklich gesetzter Endpoint wird nie ersetzt. Der alte Vorgabewert
  `http://127.0.0.1:11434` gilt nur noch bei Protokoll `ollama`; bei `openai`/`anthropic` (auch über ein Preset) ist er
  ein Validierungsfehler statt einer stillen Umleitung zur Cloud. Preset `custom` braucht einen Endpoint.
- **Lokal/Cloud:** Der Host wird mit dem `url`-Crate exakt geparst. Lokal sind nur `127.0.0.0/8`, `::1` und
  `localhost`; alles andere (und alles Unlesbare) zählt als Cloud.
- **Schlüssel:** Mit `api_key_file` ist `http://` nur zu Loopback-Hosts erlaubt (`validate()` und `call()`).
  Zugangsdaten in der URL (`user:pw@`) sind verboten. Fehlermeldungen nennen nur den Host, bei HTTP 401/403 nur
  „HTTP 401 (check API key)“.
- **Antwort:** Steuerzeichen außer `\n`/`\t` werden entfernt; hat die Antwort mehr Zeilenumbrüche als die Eingabe und der
  Modus ist nicht `email`, werden sie zu Leerzeichen (ein Return im Terminal wäre ein Befehl). Längengrenze
  `max(len*3, len+40)`.
- **cancel während der KI:** `cancel` setzt ein Abbruch-Flag, der HTTP-Client (`src/http.rs`) prüft es nach jedem
  Socket-Aufruf (Fristen 50 ms), auch bei Namensauflösung und Verbindungsaufbau. Der Aufruf kehrt nach spätestens
  ~50 ms zurück und schließt die Verbindung sofort; es läuft keine Anfrage nach.
- **Anfragegröße:** Zeilen am Socket bis 64 KB, `set_config` bis 1 MB; darüber `request too large` und die Verbindung
  schließt.
- **Downloads (GUI):** Nur mit gültiger SHA-256 (64 Hex), Abbruch über `max(erwartet, Content-Length) × 1,01`, keine
  Weiterleitung auf `http://`, Dateiname nur `[A-Za-z0-9._-]+\.bin` ohne führenden Punkt.
- **Konfiguration schreiben:** Symlinks (chezmoi) bleiben erhalten, das Verzeichnis wird nach dem Umbenennen gesynct, und
  vorher muss `Config::from_toml(&text)` die Datei wieder zur selben Konfiguration lesen.
- **Verlauf:** Gekürzt wird nur, wenn der Zähler über `max_entries` liegt; die Datei wird beim Öffnen auf 0600 gestellt.
- **Awesome:** `final` darf nur `BackSpace` als Taste tippen; ein hängendes `xdotool` wird nach 30 s (plus Tippdauer)
  per PID beendet, dann läuft die Warteschlange weiter.
