# dictate – Live-Diktat für AwesomeWM (Implementation Plan)

> **Archiv (W7, 21.11.2026):** Beschreibt den Stand unter AwesomeWM/X11; AwesomeWM, die X11-Wege und die CLIs sind
> entfernt, dictate läuft unter niri (Wayland). Gültig sind `README.md` und `PLAN-wayland.md`; der Inhalt hier bleibt
> unverändert.

Stand 22.10.2026. Lead: Hauptsitzung. Umsetzung durch Sub-Agents in drei Wellen.
Dieses Dokument ist der Vertrag zwischen den Aufgaben: Schnittstellen hier ändern nur nach Absprache mit dem Lead.

## Ziel

`Mod+e` gedrückt halten und sprechen. Ein Overlay zeigt die Wörter live (bestätigter Text normal, unsicherer Rest
grau, wie bei Claude). Beim Loslassen läuft ein letzter sauberer Durchlauf, der Text wird per `xdotool type` ins
fokussierte Fenster getippt. Komplett lokal (RTX 3060, CUDA), Deutsch mit englischen Einsprengseln.

## Ergebnisse aus dem Vortest (`/home/data/stt-test`, nicht Teil des Repos)

- Modell `ggml-large-v3-turbo-q5_0.bin` (574 MB, ~0,6 GB VRAM): Laden 270 ms, ein Durchlauf 300–600 ms für 5–30 s Audio.
  Encoder kostet immer ~215 ms, weil Whisper auf 30 s auffüllt. Das f16-Modell hat 30 s Audio einmal fast komplett
  verschluckt; q5_0 ist gesetzt.
- Ohne Spracherkennung erfindet Whisper in Pausen „Vielen Dank.“ (auch bei einem einzelnen Knacken mit −41 dBFS).
  `no_speech_prob` lieferte im Stream-Modus 0,00 und hilft nicht. **Silero-VAD** (`ggml-silero-v5.1.2.bin`, 885 kB)
  löst das vollständig. Raumrauschen am Brio-Mikrofon ≈ −77 dBFS.
- Hänger im Vortest: `whisper-stream` dekodiert alle 500 ms das ganze 8-s-Fenster; dauert ein Durchlauf länger als
  der Schritt, staut sich die Anzeige. Gegenmittel unten: nur die aktuelle Phrase dekodieren, an Pausen festschreiben,
  `audio_ctx` für Zwischenstände verkleinern, Takt adaptiv (nie zwei Durchläufe in Warteschlange).
- Mikrofon: PipeWire-Default ist `alsa_input.usb-046d_Logitech_BRIO_DF92068A-03.analog-stereo`.
  Das Sony-Headset hat kein Mikrofon (Bluetooth bewusst nur A2DP).
- Testaudio (16 kHz mono): `/home/data/stt-test/audio/de05.wav`, `de10.wav`, `de30.wav` (gesprochene Wikipedia
  „Thomas Midgley“). Referenz für de30 siehe `/home/data/stt-test/out-large-v3-turbo-q5_0-de30.txt`.

## Umgebung

- Arch Linux, X11, AwesomeWM (awesome-git), PipeWire mit pipewire-pulse.
- Rust: rustup-Toolchain in `~/.cargo/bin` (liegt im PATH vor `/usr/bin`), rustc 1.91. clang 23 vorhanden (bindgen).
- CUDA 13.4 unter `/opt/cuda` (`/etc/ld.so.conf.d/cuda.conf` vorhanden), GPU sm_86.
- Repo: `/home/git/blub/dictate` (lokales git). Kein Push ohne Freigabe des Users.
- Modelle: `~/.local/share/dictate/models/` (liegt auf /home = sdb4, nicht auf der knappen NVMe).
- Alle sichtbaren Texte (Overlay, CLI-Meldungen, Unit-Beschreibung) **Englisch**; Code-Kommentare Deutsch oder Englisch,
  README Deutsch.
- Harte Regeln aus dem System: in Awesome **nie** synchron warten (kein `io.popen`, kein `awful.spawn` mit Shell-Schleife,
  kein Polling). Nie `pkill -f` mit Mustern verwenden. Keine Transkripte ins Log, außer mit `DICTATE_DEBUG=1`.

## Architektur

```
Awesome (Lua)                       dictate daemon (Rust, systemd --user)
 Mod+e press  ── dictate start ──▶  control.sock ──▶ Engine-Thread
 Mod+e release ─ dictate stop ──▶                    ├─ Streamer (Phrasen, LocalAgreement, VAD)
 dictate listen --format awesome ◀── Events ─────────┤  └─ Engine (whisper-rs, CUDA, Silero-VAD)
 Overlay (wibox) + xdotool type                      └─ Audio-Thread (libpulse simple, 16 kHz f32, nur beim Zuhören offen)
```

Ein Binary `dictate` (Crate mit `lib` + `bin`), Unterbefehle per clap.

### Socket-Protokoll

- Socket `$XDG_RUNTIME_DIR/dictate/control.sock`, Verzeichnis 0700. Ein JSON-Objekt pro Zeile.
- Befehle: `{"cmd":"start"}`, `{"cmd":"stop"}`, `{"cmd":"cancel"}`, `{"cmd":"toggle"}`, `{"cmd":"status"}`,
  `{"cmd":"subscribe"}`, `{"cmd":"shutdown"}`.
- Antwort: `{"ok":true,"state":"…"}` bzw. `{"ok":false,"error":"…"}`. `start` im Zustand listening und `stop` im
  Zustand idle sind ok (idempotent), nicht Fehler. `subscribe` hält die Verbindung offen und streamt Events.
- Events (JSON, eine Zeile):
  - `{"event":"state","state":"idle|loading|listening|finalizing|error"}`
  - `{"event":"partial","stable":"…","tail":"…"}` – `stable` = gesamter bisher bestätigter Text der Sitzung,
    `tail` = unbestätigter Rest der laufenden Phrase.
  - `{"event":"level","db":-35.2,"speech":true}` – höchstens 10 Hz, nur während listening.
  - `{"event":"final","text":"…","cancelled":false,"audio_ms":1234,"finalize_ms":180}`
  - `{"event":"error","msg":"…"}`

### CLI

| Befehl | Verhalten |
|---|---|
| `dictate daemon` | Daemon im Vordergrund (für systemd). Eine Instanz; veralteten Socket entfernen. |
| `dictate start\|stop\|cancel\|toggle\|status\|shutdown` | Befehl senden, Antwort ausgeben. Exit 0 ok, 1 Fehler, 2 Daemon nicht erreichbar. |
| `dictate listen [--format json\|awesome]` | `subscribe`, Events zeilenweise auf stdout (flush je Zeile). Endet, wenn der Socket schließt (Exit 2 wenn nicht erreichbar). |
| `dictate transcribe FILE.wav [--stream] [--realtime]` | Offline-Test. Ohne `--stream`: ein Durchlauf, Text auf stdout. Mit `--stream`: speist die Datei in 20-ms-Blöcken durch den Streamer und gibt dieselben Events (json) aus; virtuelle Uhr, außer `--realtime`. Am Ende Statistik (Partial-Latenz p50/p95, Zahl der Durchläufe) auf stderr. |

`--format awesome` (für Lua, das kein JSON-Modul hat): Tab-getrennte Zeilen, in Textfeldern werden `\t`, `\r`, `\n`
durch Leerzeichen ersetzt:
```
state<TAB>listening
partial<TAB><stable><TAB><tail>
level<TAB>-35.2<TAB>1
final<TAB><text>            (bei cancelled: final_cancelled<TAB>)
error<TAB><msg>
```

### Konfiguration `~/.config/dictate/config.toml` (alle Schlüssel optional)

```toml
model = "~/.local/share/dictate/models/ggml-large-v3-turbo-q5_0.bin"
vad_model = "~/.local/share/dictate/models/ggml-silero-v5.1.2.bin"
language = "de"
source = ""              # Pulse-Quelle, leer = Default
threads = 4
step_ms = 350            # Mindestabstand zwischen Zwischen-Durchläufen
commit_silence_ms = 700  # Pause, nach der eine Phrase festgeschrieben wird
max_phrase_ms = 15000
idle_unload_s = 600      # Modell nach so viel Leerlauf entladen (VRAM frei), 0 = nie
final_beam_size = 5
initial_prompt = ""
trailing_space = true
blocklist = ["Vielen Dank.", "Vielen Dank fürs Zuschauen.", "Untertitel im Auftrag des ZDF", "Untertitelung", "Amara.org", "Thanks for watching!", "Für You in God."]
```

### Modul-Aufteilung (Datei-Besitz je Aufgabe, damit parallele Agents sich nicht überschreiben)

| Datei | Besitzer |
|---|---|
| `Cargo.toml`, `src/main.rs` (komplettes clap-Gerüst mit allen Unterbefehlen, ruft Modulfunktionen), `src/lib.rs`, `src/config.rs`, `src/engine.rs`, `src/wav.rs`, Stubs für alle anderen Module | T1 |
| `src/stream.rs`, `src/filter.rs`, `src/simulate.rs` (`transcribe --stream`), `tests/stream_*.rs` | T2 |
| `src/audio.rs`, `src/daemon.rs`, `src/protocol.rs`, `src/client.rs`, `systemd/dictate.service`, `tests/daemon_*.rs` | T3 |
| `awesome/dictate/init.lua`, `awesome/test/` (Test-rc, Fake-CLI) | T4 |
| `README.md`, Installation, rc.lua-Einbindung, chezmoi | Lead (T5) |

`main.rs` gehört nach T1 niemandem mehr; muss T2/T3 dort etwas ändern, nur minimal und im Bericht nennen.

### Schnittstellen (von T1 anzulegen, von T2/T3 zu benutzen)

```rust
// engine.rs
pub struct Segment { pub text: String, pub t0_ms: i64, pub t1_ms: i64, pub no_speech_prob: f32 }
pub struct DecodeOpts<'a> { pub audio_ctx: Option<i32>, pub beam_size: usize, pub prompt: &'a str, pub single_segment: bool }
pub trait Transcriber {
    fn transcribe(&mut self, pcm: &[f32], opts: &DecodeOpts) -> anyhow::Result<Vec<Segment>>;
    /// Sprachabschnitte als Sample-Bereiche (16 kHz) laut Silero-VAD
    fn speech_ranges(&mut self, pcm: &[f32]) -> anyhow::Result<Vec<std::ops::Range<usize>>>;
}
pub struct Engine { … }               // impl Transcriber; Engine::load(&Config) -> Result<Engine>; Drop gibt VRAM frei
// stream.rs (T2)
pub struct Partial { pub stable: String, pub tail: String }
pub struct Streamer { … }
impl Streamer {
    pub fn new(cfg: &Config) -> Self;
    pub fn push(&mut self, pcm: &[f32]);                          // 16 kHz mono f32
    pub fn due(&self) -> bool;                                    // lohnt ein Zwischen-Durchlauf jetzt?
    pub fn tick(&mut self, t: &mut dyn Transcriber) -> anyhow::Result<Option<Partial>>;
    pub fn finish(&mut self, t: &mut dyn Transcriber) -> anyhow::Result<String>; // Restphrase final, Gesamttext
    pub fn reset(&mut self);
}
```

Der Daemon (T3) ruft `push` mit jedem Audioblock und `tick` immer dann, wenn `due()` und der Engine-Thread frei ist –
nie mehr als ein Durchlauf gleichzeitig, kein Rückstau.

## Streaming-Algorithmus (T2, Kern der Qualität)

1. Puffer `phrase` = Audio seit dem letzten Festschreiben. Führende Stille über 1 s wird verworfen (bis auf 300 ms Vorlauf).
2. `tick`: VAD über die Phrase. Keine Sprache → nichts tun. Sonst Zwischen-Durchlauf über die Phrase mit
   `single_segment`, kleinem `audio_ctx` (proportional zur Länge plus Rand, Untergrenze ausmessen; nur für
   Zwischenstände) und `prompt` = letzte ~200 Zeichen bestätigter Text.
3. **LocalAgreement-2**: Wortweise längster gemeinsamer Präfix der letzten zwei Hypothesen = bestätigt innerhalb der
   Phrase; was einmal bestätigt ist, schrumpft nicht mehr. Rest = `tail`.
4. **Festschreiben**, wenn nach Sprache `commit_silence_ms` Stille folgt oder die Phrase `max_phrase_ms` erreicht
   (dann an der letzten VAD-Pause schneiden, sonst alles): finaler Durchlauf über nur die Sprachbereiche der Phrase,
   volles `audio_ctx`, `final_beam_size`, Prompt wie oben. Ergebnis filtern, an den Gesamttext hängen, Phrasenpuffer leeren
   (Audio nach dem Schnitt behalten).
5. `finish`: Restphrase wie in 4. festschreiben (falls Sprache), Gesamttext normalisieren (Leerzeichen, Trim) zurückgeben.
6. Filter (`filter.rs`): Phrase verwerfen, wenn sie nach Normalisierung (Kleinschreibung, Satzzeichen) nur aus einem
   Blocklist-Eintrag besteht; Segmente mit `no_speech_prob > 0.6` verwerfen; Wiederholungsschleifen (dasselbe n-Gramm ≥ 4×)
   kürzen.
7. `due()`: neue Sprache seit letztem Durchlauf und ≥ `step_ms` vergangen, oder Festschreiben fällig.

Messziel (auf `de10`/`de30`, `transcribe --stream`): Zwischenstand-Latenz p95 < 700 ms, Finalisierung nach
`stop` < 400 ms für eine ≤ 10-s-Phrase, Endtext von `--stream` mindestens so gut wie ein Batch-Durchlauf.

## Aufgaben und Wellen

| Welle | Aufgabe | Modell | Inhalt |
|---|---|---|---|
| 1 | **T1 Kern** | Sonnet | Crate-Gerüst, whisper-rs mit `cuda` bauen (prüfen, ob 0.16 die VAD-Felder bzw. `whisper_vad_*` hergibt, sonst über `whisper-rs-sys`), `Engine` + `Transcriber`, Config, WAV-Lesen, `transcribe FILE` (Batch), Modelle nach `~/.local/share/dictate/models/` kopieren, alle anderen Module als kompilierende Stubs. Messung: Batch-Zeit de05/de10/de30. |
| 1 | **T4 Awesome** | Sonnet | Lua-Modul mit Overlay, Tastenlogik, Listener, xdotool-Ausgabe; Test in Xephyr mit Minimal-rc und Fake-`dictate`. Fasst die echte rc.lua **nicht** an und startet die echte Awesome-Sitzung nicht neu. |
| 2 | **T2 Streaming** | Opus | `stream.rs`, `filter.rs`, `transcribe --stream`, Unit-Tests mit Mock-Transcriber, `#[ignore]`-Integrationstests mit echtem Modell, Messung gegen die Messziele, `audio_ctx`-Untergrenze ausmessen. |
| 2 | **T3 Daemon** | Sonnet | Audio (libpulse simple, nur beim Zuhören geöffnet), Daemon (Threads, Socket, Broadcast, Laden/Entladen, Zustände), Client-Befehle, `listen` in beiden Formaten, systemd-User-Unit. Test mit Mock-Streamer bzw. echtem Streamer, sobald T2 liefert; Ende-zu-Ende über eine virtuelle Pulse-Quelle (null-sink + `paplay`). |
| 3 | **T5 Integration** | Lead | Installation (`cargo install --path .`), Unit aktivieren, rc.lua-Einbindung, Awesome neu laden, E2E-Test, README, chezmoi. |
| 3 | **Review** | Sonnet | Code-Review des gesamten Repos und der rc.lua-Änderung; der Lead entscheidet über Fixes. |

## Awesome-Modul (T4) im Detail

- Datei `awesome/dictate/init.lua` im Repo; der Lead verlinkt/kopiert sie später nach `~/.config/awesome/dictate/`
  (chezmoi). Kein Zugriff auf andere Awesome-Dateien.
- API: `local dictate = require("dictate")`, `dictate.setup{ cmd = "dictate", ... }`, `dictate.keys(modkey)` gibt eine
  mit `gears.table.join` gebaute Key-Liste zurück (rc.lua verbindet sie mit `globalkeys`, **nicht**
  `awful.keyboard.append_global_keybindings`). Hotkey-Gruppe `"\u{F14DE} Launch"` (existiert in `themes/qubit/hotkeys.lua`).
- Tasten: `Mod+e` halten = sprechen (press → `dictate start`, release → `dictate stop` mit ~80 ms Entprellung, weil
  X-Autorepeat beim Halten Release/Press-Paare schickt; in Xephyr per `xdotool keydown/keyup` und echtem Halten
  nachweisen). `Mod+Shift+e` = Freihand-Umschalter (`toggle`). `Mod+Ctrl+e` = abbrechen (`cancel`).
- Listener: `awful.spawn.with_line_callback({cmd, "listen", "--format", "awesome"}, …)`; bei Ende Neustart mit
  Backoff 1, 2, 4 … 30 s. Kein Polling.
- Overlay: `wibox`, ontop, unten mittig auf `awful.screen.focused()`, Breite ≤ 60 % des Screens, abgerundet, Farben und
  Schrift aus `beautiful` (Theme qubit, Fallbacks). Kopfzeile mit Zustand („● Listening“, „Loading model…“,
  „Finishing…“) und schmalem Pegelbalken aus `level`; darunter Text: `stable` normal, `tail` gedimmt
  (Pango-Markup, immer `gears.string.xml_escape`). Höchstens ~3 Zeilen, ältester Text fällt vorne weg. Wird einmal gebaut
  und wiederverwendet.
- `final`: Overlay ausblenden; Text nicht leer → `awful.spawn({"xdotool","type","--clearmodifiers","--delay","6","--", text})`
  (Argumentliste, keine Shell). `error`: Overlay zeigt Meldung 3 s.
- Fehlender Daemon: Overlay meldet „Dictation daemon not running“ beim Drücken.
- `M._debug()` liefert den Zustand für `awesome-client`.

## Abnahme (Definition of Done)

1. `cargo build --release` und `cargo test` grün, `cargo clippy` ohne Warnungen.
2. `dictate transcribe de30.wav --stream` erreicht die Messziele; Ergebnis im Bericht.
3. Daemon läuft als `systemctl --user`-Dienst, Leerlauf ohne CPU-Last (0 Ticks / 30 s), VRAM nach `idle_unload_s` frei.
4. Über die virtuelle Quelle: `start` → Audio → `stop` liefert den erwarteten Text als `final`-Event.
5. In Awesome: `Mod+e` halten zeigt live Text, Loslassen tippt ihn ins fokussierte Fenster; keine Startfehler
   (`awesome.startup_errors` leer, `awesome.conffile` = eigene rc.lua).
6. Der User testet live mit seiner Stimme.

## Berichtsformat der Sub-Agents

Am Ende jeder Aufgabe ≤ 40 Zeilen: was gebaut, welche Dateien, Commits (Hash), Messwerte, Abweichungen vom Plan,
offene Punkte. Große Logs in Dateien unter `/home/data/stt-test/logs/`, nicht in den Bericht.
