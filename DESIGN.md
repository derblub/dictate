# dictate – Design (Noctalia)

Verbindlich für alles Sichtbare von dictate unter niri: Overlay, Panels und Befehlspalette in `dictate-ui` (GTK 4) und
das Einstellungsfenster `dictate-settings` (egui). Grundlage ist die Entscheidung vom 20.11.2026 (Entscheidung 9 in
`PLAN-wayland.md`): dictate richtet sich nach der Palette, die Noctalia auch für niri, Alacritty und niri-coach
erzeugt; das frühere Design nach dem Pushing Pixels Design Guide (Tokens, Versalien, Ecken 0, Glühen) entfällt und
steht nur noch in der Git-Geschichte (bis W6, Commit vor Block E). Herleitung der Einzelheiten: `PLAN-wayland.md` 6h
(Regeln 1–8, Lesarten L40–L53, Entscheidungen 20 und 21).

## Grundlinie

Ruhig und eingebettet: dictate sieht aus wie der Rest des Desktops. Farben nur aus den Material-Rollen der Palette,
Schrift aus den GTK-Einstellungen, Ecken wie Noctalias Panels. Keine Schatten, kein Glühen, keine Verläufe, keine
Emoji. Einziges eigenes Motiv ist das Pixel (ein Quadrat): die Pixelwelle im Overlay, die Pixel-Säulen und -Meter und
das Zeichen im Logo. Ein Akzent (`primary`).

## Palette

- **Quelle:** Noctalia schreibt bei jedem Wechsel von Schema oder Modus über die Vorlage `noctalia/dictate.json.tmpl`
  (eingetragen in `~/.config/noctalia/config.toml` unter `[theme.templates.user.dictate]`) die Datei
  `~/.local/state/dictate/palette.json` (`$XDG_STATE_HOME`, im Test `DICTATE_PALETTE`). Einzige Quelle für Pfad,
  Parser und Rückfall ist `src/ui/palette.rs`; das Einstellungsfenster bindet es per `#[path]` ein (Regel 2).
- **Fremde Eingabe:** gelesen wird nur eine gewöhnliche Datei bis 64 KiB; fehlt sie, ist sie unlesbar oder fehlt eine
  der zehn gelesenen Rollen, gilt die eingebaute Palette **Tokyo Night dunkel** ganz (Grund im Log, nie Absturz).
- **Live:** beide Programme beobachten den Ordner der Datei (Ersetzen per `rename`, Löschen, Neuanlage; Entprellung
  50 ms) und färben ohne Neustart um. Kein Neuzeichnen im Leerlauf.
- **Hell/Dunkel** nur aus `mode` der Palette, nie aus Portal oder gsettings (Regel 3, Vorlage V2).
- **Nur die 28 Rollen der Vorlage** (dieselbe Liste wie niri-coach); eine neue Rolle hieße neue Vorlage über chezmoi.
  Eine Rolle `outline` gibt es nicht, nur `outline_variant`.
- Die Häufigkeit der Befehlspalette liegt daneben in `frecency.json`, nie unter dem Namen `palette.json`
  (Entscheidung 21).

### Rollen und ihre Verwendung

| Rolle | `dictate-ui` (Overlay, Panels) | Einstellungsfenster |
|---|---|---|
| `surface` | Karte des Overlays (mit eingestellter Deckkraft), Grund der Panels | Inhalt und Aktionsleiste |
| `surface_container_low` | – | Seitenleiste, Bildschirm der Overlay-Vorschau |
| `surface_container` | – | Popups, Hinweisleiste „Daemon not running“ |
| `surface_container_high` | – | Felder, Auswahllisten, Zahlenfelder, Hover in Popups |
| `on_surface` | Text, Ruheton der Welle | Text, leere Zelle (12 %), Pegel ohne Sprache (50 %) |
| `on_surface_variant` | unbestätigter Tail, Nebentext, Gruppenlabels | Nebentext, Feldlabels, Platzhalter, Gruppenlabels, Hinweise |
| `outline_variant` | Haarlinie am Rand (1 px), Rahmen von Chips und Zweitknöpfen, Vorschau- und Codeflächen (45 %) | Rahmen, Trennlinien, Fensterrand der Popups |
| `primary` | Mittelzeile der Welle, wartender Pixel, aktive Zeile (16 %), Hauptknopf, Fokus, Akzent-Labels, Pseudonyme | Akzent: Links, gewählter Chip (Rahmen und Text, Fläche 10 %), aktive Zeile der Seitenleiste (16 %), Auswahl (24 %), Hover-Rahmen (40 %), Fokus, Hauptknopf, Kürzel |
| `on_primary` | Text auf dem Hauptknopf und im angekreuzten Kästchen | Text auf dem Hauptknopf, Zustandsschicht beim Hover (8 %) |
| `error` | Fehlertext, Rahmen bei gefundenen Geheimnissen | Fehlermeldungen unter Feldern, Fehlerrahmen |

Der Zwischenton der Welle (Abstand 1 zur Mitte) ist halb `primary`, halb `on_surface` (`Palette::mix`).
Deckkräfte als Konstanten: `src/gui/theme.rs` (`CHIP_FILL`, `ROW_FILL`, `SELECTION_FILL`, `HOVER_STROKE`,
`STATE_LAYER`), Töne der Welle in `src/ui/wave.rs` (`OFF_ALPHA` 12 %, `DASH_ALPHA` 40 %). Das Einstellungsfenster
liest Farben nur über `theme::th()`; `Theme` ist die einzige Stelle, an der Rollen zu Farben werden.

Auf gefüllten Akzentflächen steht in beiden Programmen `on_primary`, nie `surface` (Entscheidung 22; Test
`text_on_the_accent_is_on_primary` in `src/bin/dictate-ui/panel.rs`).

### Kontrast

Regel 5 nach Entscheidung 21: jede Textpaarung, die das Einstellungsfenster zeichnet, hat bei Tokyo Night dunkel
mindestens 4,5:1 (`Theme::text_pairs`, Test `text_pairs_have_contrast`; wer ein Widget mit neuer Paarung baut, trägt
sie dort ein). Helle Schemata werden nur gemessen (Ausgabe des Tests, Messpunkt M5); die Farben bleiben exakt wie
Noctalia sie liefert, auch wenn ein Paar darunter liegt. Text steht in `on_*`-Rollen auf ihrer Fläche, Nebentext in
`on_surface_variant`; dazu `primary` (Akzenttext) und `error` (Fehlertext) auf `surface` wie bei Material 3 (L49).
Abgeschaltete Bedienelemente (30 % bzw. 50 %) treten bewusst zurück und sind ausgenommen.

## Schrift

- **Aus den GTK-Einstellungen** (L41, Messpunkt M1): Quelle ist nur `gtk-font-name` in
  `$XDG_CONFIG_HOME/gtk-4.0/settings.ini` (sonst `~/.config/gtk-4.0/settings.ini`), wie GTK 4 außerhalb einer
  Sandbox; kein gsettings, kein Portal. Fehlt sie, gilt wie bei GTK `Adwaita Sans 11`. Die Pango-Beschreibung wird
  zerlegt (letztes Wort = Größe in pt oder `px`, Stil- und Gewichtswörter fallen weg), nur die Familie geht an
  `fc-match`; Größe in logischen Pixeln = pt × dpi / 72 mit dpi aus `gtk-xft-dpi` / 1024 (Vorgabe 96). Einzige Quelle
  im Einstellungsfenster: `fonts::gtk_font`. Auf blub: Roboto 12, also 16 px. `fc-match` hat beim Start eine
  Gesamtfrist von 10 s (`fonts::FC_DEADLINE`); danach sucht `fonts::fallback_face` verbreitete Sans-Dateien ohne
  fontconfig (egui bringt seit U2 keine eigene Schrift mit, `default_fonts` aus), das Fenster geht trotzdem auf; ohne
  jede Schrift endet es mit einer Meldung.
- Mono ist `monospace` über fontconfig (wie `font-family: monospace` im Panel), nur für Code, Klassennamen, Pfade,
  Kürzel-Chips und Berichte.
- **Größen als Verhältnis zur Basisschrift** wie im Panel (L40, L50; `theme::Size`): 1 em Fließtext, Feldlabels,
  Seitenleiste, Knöpfe und Chips (Gewicht 400); 0,85 em Hinweise, Meldungen unter Feldern, Textknöpfe in Tabellen
  und Mono; 0,8 em Gruppenlabels (Gewicht 600, `on_surface_variant`); 1,1 em Seitentitel im Fenster und Überschriften im Panel
  (Gewicht 600). Der Kopf des Panels (`.dp-title`) ist 0,85 em, 600, `on_surface_variant`.
- **Keine Versalien, keine Laufweite** im Einstellungsfenster. Das Panel von `dictate-ui` hat bei Titel und
  Gruppenlabels noch `letter-spacing: 1px` (vermerkte Abweichung, L50).
- Gewicht 600: dieselbe Datei mit Achse `wght`, sonst die statische Datei, die fontconfig für `semibold` nennt.

## Form und Raum

- **Ecken** (L42, Konstanten in `src/ui/palette.rs`): `RADIUS` 12 für Overlay, Panels, Popups und das Fenster selbst,
  `RADIUS_CONTROL` 8 für Felder, Knöpfe und Zeilen, `RADIUS_CHIP` 4 für Chips und Kästchen; Pixel-Zellen 1. Die
  Fenster rundet niri über Regeln in `niri/dictate-binds.kdl` (`geometry-corner-radius 12`, `clip-to-geometry true`
  für `dictate-panel` und `dictate-settings`, `layer-rule` für das Overlay); KDL und Binary kommen nur zusammen auf
  blub (Regel 7).
- **8-px-Raster** für Abstände und Innenabstände (Ausnahmen: Pixel-Raster 8 auf 11, Fokusring 3 px).
- Keine Schatten. Rahmen 1 px `outline_variant`.
- **Fokus:** 2 px `primary`, im Einstellungsfenster 3 px außerhalb der Form mit mitwachsenden Ecken, im Panel
  `outline-offset: 2px`.
- **Zustände:** Hover über Zeilen und Zweitknöpfen als `primary` 16 % (Panel) bzw. Rahmen `primary` 40 %
  (Einstellungsfenster); Hover über dem Hauptknopf als Zustandsschicht `on_primary` 8 % (L53). Kein Glühen.

## Overlay (`dictate-ui`)

- Karte `surface` mit der Deckkraft aus den Einstellungen (Transparenz aus = deckend), 1 px `outline_variant`, Ecken
  12, Innenabstand 16 px oben und unten, 24 px seitlich; Breite wächst bis `overlay.max_width` (Vorgabe 720, gekappt
  auf 90 % des Bildschirms), dann Umbruch, höchstens 3 Zeilen; Text `on_surface`, Tail `on_surface_variant`.
- **Pixelwelle** (`src/ui/wave.rs`, einzige Quelle für Overlay und Vorschau): 7 Säulen, symmetrisch um die
  Mittelzeile, Höhen 1, 3 oder 5 Zellen, Zelle 6 px auf 8 px Raster, Ecke 1 px. Jede Säule hat ein festes Gewicht,
  Höhe = Gewicht × geglätteter Pegel. Mittelzeile `primary`; neue Zellen erscheinen im Akzent und gehen mit jedem
  Pegel-Event eine Stufe zu ihrem Ruheton (Mischung, dann `on_surface`). Warten (Laden, Endverarbeitung, Hinweise):
  leere Zellen (`on_surface` 12 %), die letzte im Akzent. Fehler: die letzte als gestrichelter Slot (40 %).
- Gezeichnet wird nur bei Pegel-Events und Zustandswechseln, kein Animations-Timer. Keine Statustexte.

## Panels und Befehlspalette (`dictate-ui`)

GTK-Fenster mit `app_id` `dictate-panel`, CSS aus den Rollen (`src/bin/dictate-ui/panel.rs`, `panel_css`): Grund
`surface`, Rand 1 px `outline_variant`, Ecken 12; Kopf mit Titel in `on_surface_variant` (0,85 em, 600), Zeilen
40 px hoch mit Ecken 8, aktive Zeile `primary` 16 %, Kürzel als Chip (Mono 0,85 em, Rahmen `outline_variant`, Ecken
4), Hauptknopf `primary` mit Text `on_primary`, Zweitknopf mit Rahmen, Vorschau- und Codeflächen `outline_variant` 45 %, Geheimnis-Warnung
mit Rahmen `error`. Markdown aus KI-Antworten: Überschriften 1,1 em 600, Zitat mit 3-px-Linie links.

## Einstellungsfenster (egui)

- Fenster 1180 × 860, schwebend (Fensterregel), Seitenleiste links 240 px auf `surface_container_low`, Inhalt auf
  `surface` mit 48 px Rand, höchstens 720 px breit. Seitenleiste: Logo (16 px), Einträge 1 em in
  `on_surface_variant`, aktiver Eintrag `on_surface` auf `primary` 16 % mit Ecken 8; unten der Zustand des Daemons als
  Pixel (ohne Glühen).
- Kopf je Seite: Titel 1,1 em, 600, `on_surface`, darunter ein Satz Erklärung in `on_surface_variant`, 32 px Abstand.
- **Gruppen** durch Label (0,8 em, 600, `on_surface_variant`) und 32 px Abstand getrennt, keine Kästen.
- **Felder:** Grund `surface_container_high`, 1 px `outline_variant`, Ecken 8; Hover Rahmen `primary` 40 %, aktiv
  `primary`. Label 1 em `on_surface_variant` darüber, Hinweis und Fehlermeldung 0,85 em darunter (Fehler in `error`,
  Rahmen dann `error`), 16 px bis zum nächsten Feld. Warnungen sind nicht blockierend und darum Nebentext, nicht rot.
- **Chips** statt runder Schalter für Ein/Aus und Auswahl: Rahmen 1 px `outline_variant`, Text `on_surface_variant`,
  Ecken 4; Hover Rahmen `primary` 40 %; gewählt Rahmen und Text `primary`, Fläche `primary` 10 %.
- **Slider:** Spur 8 px rund in `outline_variant`, Füllung und Daumen (24 px rund) `primary`.
- **Knöpfe:** Hauptknopf („Apply“) gefüllt `primary`, Text `on_primary`, wie geschrieben (keine Versalien),
  abgeschaltet 30 % Deckkraft; Zweitknopf („Revert“, „Test“) 1 px `outline_variant`, Text `on_surface`. Textknöpfe in
  Tabellen 0,85 em ohne Rahmen.
- **Tabellen** (Vokabular, Profile, Verlauf): Zeilen durch 1 px `outline_variant` getrennt, keine Zebrastreifen.
- **Pixel-Meter** (Modell-Download): 8 Zellen 8 px auf 11 px, gefüllt `primary`, leer `on_surface` 12 %.
  **Pegel** (Seite Audio): 5 Säulen à 3 Zellen im selben Raster.
- **Overlay-Vorschau** (Seite Overlay, L45, L52): Bildschirm (als 1920 × 1080 angenommen) und Overlay im selben
  Maßstab, so breit wie die Inhaltsspalte (bei 720 Breite 0,375, also 405 hoch). Bildschirm `surface_container_low`
  mit Ecken 8; Karte `surface` mit eingestellter Deckkraft, Rahmen `outline_variant`, Ecken 12, oben oder unten mit
  dem eingestellten Abstand (ein Abstand, der das Overlay aus dem Bild schöbe, hält es am Rand). Gezeigt wird das
  Overlay bei langer Mitschrift (größte Breite, zwei Zeilen); Text als Balken (`on_surface`, Tail
  `on_surface_variant`), weil Schrift in diesem Maßstab unleserlich wäre. Welle mit festen mittleren Pegeln, kein
  Zeitgeber. Maße, Welle und Töne aus `src/ui/wave.rs`, nicht abgeschrieben.
- Hinweisleiste „Daemon not running …“ auf `surface_container`, Text `on_surface`; kein Gelb.

## Logo und Marke

- **Zeichen von dictate** (V3, L53): ein Pixel-Feld aus 5 × 3 Zellen, die Mittelreihe als Signallinie, genau ein
  Pixel (Spalte 4 der Mittelreihe) im Akzent. Im Einstellungsfenster aus Rollen gezeichnet statt als Bild
  (`widgets::logo`): Zellen `on_surface` 12 %, Mittelreihe 50 %, Akzentzelle `primary`; daneben die Wortmarke
  „dictate“ als Text in der Titelschrift (Gewicht 600). Maße, Abstand, Grundlinie und Laufweite folgen
  `assets/logo/lockup-on-dark.svg` relativ zur Höhe des Zeichens (Test `logo_follows_the_svg` hält Zellen und Maße
  gleich den SVGs). Seitenleiste 16 px, About 36 px. Varianten mit Wellenformen und geschobenem Pixel sind verworfen
  (22.10.2026).
- **Programm-Icon** bleibt fest: `assets/logo/icon.svg` und `icon-512.png` (Fenster-Icon, eingebunden in
  `src/bin/dictate-settings.rs`; nach Änderung am SVG das PNG neu rendern). Icons werden nicht umgefärbt.
  `assets/logo/glyph-on-dark.svg` und `lockup-on-dark.svg` sind die Vorlage für Zeichen und Lockup.
- **Pushing Pixels** (V4): die Seite About nennt nur Text und Link („Daniel Kurdoghlian“, „Pushing Pixels, Vienna“,
  `pushingpixels.at`). Das Zeichen der Marke wird nirgends gezeigt und nie nachgezeichnet oder umgefärbt.

## Texte

- Sichtbare Texte nur **Englisch** (Entscheidung des Users 22.10.2026), keine i18n-Schicht. Doku, Kommentare und
  Commits deutsch.
- Stimme: knapp, klar, technisch. Erst sagen, dann aufhören. Kurze Sätze. Ich-Form, wo ein Autor spricht (Support).
- Keine Gedankenstriche (weder – noch —), Bindestrich nur, wo die Grammatik ihn verlangt. Keine Emoji.
- Verboten: „Dive into“, „Unleash“, „Elevate“, „Seamlessly“, „Cutting-edge“, „Harness the power“.
- Überschriften, Labels und Knöpfe in Satzschreibung wie geschrieben, keine Versalien.
- Farben in Texten nach Rolle benennen („accent color“, „error color“), nie nach einem Farbnamen eines Schemas.
- Support-Seite ohne Werbesprache: ein Satz, wer das baut und dass es frei bleibt, dazu der Link.
