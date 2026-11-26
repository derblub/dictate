# Parakeet gegen Whisper: WER roh/normalisiert und Tempo (26.11.2026)

Anlass: Ein Leser behauptet, Parakeets *rohe* WER sei nur wegen Zeichensetzung und Groß-/Kleinschreibung schlechter; normalisiert sei Parakeet
deutlich besser (seine Zahlen: 11,6 % gegen 15,6 % für Whisper large-v3) und etwa 30× Echtzeit gegen 20× bei Whisper turbo.
Frage: Lässt sich das mit den 18 deutschen Testsätzen von dictate bestätigen? Nur gemessen, kein Produktcode geändert.

Messung am selben Tag wie `/home/data/stt-test/parakeet-vs-whisper-2026-11-26.md` (Methodik, Hardware, Speicher, Live-Partials dort), hier mit neuen
Läufen und neuer Auswertung. Rechner: i7-7700K (4 Kerne), RTX 3060 12 GB, CUDA 13.5, 4 CPU-Threads.

## Antwort: teilweise bestätigt

- **Tempo: bestätigt, bei Parakeet sogar deutlicher.** GPU: 77× Echtzeit (Parakeet) gegen 19× (Whisper turbo q5_0, so nutzt es dictate) und 10× (large-v3).
- **Genauigkeit: nicht bestätigt.** Auf diesen 18 Sätzen ist Parakeet *normalisiert nicht besser* als Whisper turbo (7,7 % gegen 6,8 %) und gleichauf mit large-v3
  (7,7 % gegen 7,7 %, nur deutsche Sätze). Der Unterschied liegt in jeder Auswertung innerhalb des Rauschens (paarweises Bootstrap, 95 %-Intervall der
  Differenz Parakeet minus turbo: −4,4 bis +6,0 Prozentpunkte).
- **Die Erklärung „roh schlechter wegen Zeichensetzung/Großschreibung“ passt hier nicht:** Parakeet v3 gibt Zeichensetzung und Großschreibung selbst aus. Roh
  liegen beide gleichauf (15,3 bis 15,8 %). Was roh trennt, sind Schreibweisen (`Feature-Flag` gegen `Feature Flag`, `14.30` gegen `14:30`, `1249` gegen `1.249`,
  Satzzeichen vor Sprachbefehlen), nicht fehlende Zeichensetzung. Ein Nachbearbeitungsschritt für Zeichensetzung (optional im Auftrag) entfällt darum: es gibt
  nichts nachzuholen.

## WER (gepoolt: Σ Fehler / Σ Referenzwörter; (a) roh, (b) Kleinschreibung + Satzzeichen weg, (c) nur Satzzeichen weg, Großschreibung bleibt)

Normalisierung identisch für alle Kandidaten (Skript `skripte/score3.py`): NFKC; Bindestriche → Leerzeichen; sonst nur Satzzeichen entfernt, Umlaute bleiben.
Zahlen werden *nicht* umgeschrieben (Zahlwort gegen Ziffer zählt als Fehler); stattdessen zweite Zeile „ohne Formatsätze“, siehe unten. (a) zählt
`Feature-Flag` als ein Wort, darum 216 statt 221 Referenzwörter.

**Alle 18 Sätze (221 Wörter)**

| Kandidat | (a) roh | (b) normalisiert | (c) Großschreibung bleibt |
|---|---|---|---|
| Whisper turbo q5_0, GPU (dictate heute) | 15,3 % | **6,8 %** | **9,0 %** |
| Whisper turbo fp16, GPU | 15,7 % | 6,8 % | 8,6 % |
| Whisper large-v3 fp16, GPU | 20,4 % | 12,2 % * | 15,4 % * |
| Whisper turbo q5_0, CPU | 17,1 % | 7,2 % | 9,5 % |
| Parakeet v3 fp32, GPU und CPU (gleicher Text) | 15,7 % | 7,7 % | 10,0 % |
| Parakeet v3 int8, CPU | 19,0 % | 9,0 % | 10,4 % |

\* large-v3 *übersetzt* den englischen Satz t06 ins Deutsche, weil dictates Einstellung `language = de` erzwungen ist (13 Fehlwörter), und lässt in t12 die
Sprachbefehle weg. Das ist ein Effekt der Sprachvorgabe auf einem englischen Satz, kein Maß für deutsche Erkennung. Darum die nächste Tabelle.

**Nur deutsche Sätze ohne t06 (17 Sätze) / zusätzlich ohne Formatsätze t01, t04, t10, t11, t12 (12 Sätze, 161 Wörter)**
(Formatsätze: die Referenz ist gesprochene Form, „eins, zwei, drei“, ausgeschriebene Zahlen oder Sprachbefehle; dictate setzt sie erst *nach* der Erkennung um.)

| Kandidat | 17 Sätze (a) | (b) | (c) | 12 Sätze (a) | (b) | (c) |
|---|---|---|---|---|---|---|
| Whisper turbo q5_0, GPU | 15,3 % | 7,2 % | 9,1 % | 6,4 % | **1,2 %** | 3,1 % |
| Whisper turbo fp16, GPU | 15,8 % | 7,2 % | 8,7 % | 7,1 % | 1,2 % | 2,5 % |
| Whisper large-v3 fp16, GPU | 15,3 % | 7,7 % | 9,1 % | 6,4 % | 1,2 % | 2,5 % |
| Parakeet v3 fp32 | 15,8 % | 8,2 % | 10,1 % | 9,0 % | 4,3 % | 6,2 % |
| Parakeet v3 int8, CPU | 19,2 % | 9,6 % | 10,6 % | 15,4 % | 8,1 % | 8,7 % |

(Die Zeilen „12 Sätze“ wurden nach Sichtung der Texte gewählt; sie begünstigen Whisper, weil Parakeet hier „Dämon“, „Autosmodul“, „Großglock noch“ falsch hat. Alle Zahlen
je Auswertung in `rohdaten/scores3.md`.)

## Tempo (21 Clips, 155 s Audio: t01–t18 und drei Ausschnitte; ein Lauf nach Warm-up, Latenz nur der Erkennungsaufruf)

| Kandidat | Median | p90 | Echtzeit­faktor (RTF) | × Echtzeit | Laden | Warm-up |
|---|---|---|---|---|---|---|
| Whisper turbo q5_0, GPU | 358 ms | 406 ms | 0,053 | 19× | 0,3 s | 0,4 s (1 Clip) |
| Whisper turbo fp16, GPU | 381 ms | 451 ms | 0,057 | 18× | 0,6 s | 0,4 s |
| Whisper large-v3 fp16, GPU | 597 ms | 933 ms | 0,097 | 10× | 0,9 s | 0,5 s |
| Parakeet v3 fp32, GPU | **92 ms** | **109 ms** | 0,013 | **77×** | 1,2 s | 2,9 s (ganzer Durchlauf) |
| Whisper turbo q5_0, CPU | 27,5 s | 42,4 s | 4,34 | 0,2× | 0,2 s | 39,7 s |
| Parakeet v3 fp32, CPU | 421 ms | 704 ms | 0,075 | 13× | 1,5 s | 11,5 s |
| Parakeet v3 int8, CPU | 362 ms | 512 ms | 0,060 | 17× | 1,2 s | 9,1 s |

Warm-up: Whisper ein Durchlauf über t01 (fixe Encoder-Form); Parakeet ein voller Durchlauf über alle 21 Clips (wechselnde Formen), verworfen. Die CPU-Zeile
von Whisper ist der Lauf von 11:13 desselben Tages (nicht wiederholt, 11 Minuten je Lauf); die Latenzen der GPU-Läufe liegen innerhalb weniger Prozent der Läufe vom
Vormittag (101 ms/429 ms Mittel dort, 96 ms/389 ms hier). Nicht gemessen: Whisper large-v3 auf der CPU.

## Einschränkungen und Konfidenz

- **18 Sätze, 221 Wörter, ein Sprecher, vorgelesen:** Jeder Fehler sind 0,45 Prozentpunkte. Das Intervall der Differenz Parakeet minus turbo (−4,4 bis +6,0 PP) schließt
  null ein; Rangfolgen innerhalb von etwa 5 PP sind nicht belegt. Gegen large-v3: Differenz −4,6 PP, Intervall −17,9 bis +4,5 PP (stark von t06 getrieben).
- Keine Zahlennormalisierung: bei t01, t04, t10 zählen Ziffer gegen Zahlwort als Fehler (Parakeet schreibt bei t01/t10 Wörter, Whisper Ziffern). Das nützt Parakeet in (b)/(c)
  auf allen 18 Sätzen; die Zeile ohne Formatsätze nimmt es heraus.
- Der Leser verglich vermutlich auf einem anderen Datensatz (öffentlich, mehr Sprecher und Wörter) und mit Whisper large-v3 in anderer Laufzeit; unsere Zahlen widerlegen
  seine nicht, sie bestätigen sie aber auch nicht. Ein Test mit mehr Material (z. B. Common Voice DE, MLS DE) wäre nötig, um die WER-Aussage wirklich zu prüfen; nicht gemessen.
- Whisper läuft mit dictates Einstellungen (`language = de`, greedy, ohne Vokabel-Prompt, ohne VAD); Parakeet hat keine Sprachvorgabe (t02 „Ja.“ → „Yeah.“).
- Nicht gemessen: Rauschen, mehrere Sprecher, Stille-Halluzinationen, Wechsel der Sprache im Satz.

## Sätze zur Verwendung (sinngemäß)

> Ich habe es mit 18 deutschen Testsätzen nachgemessen: Beim Tempo stimmt es, Parakeet läuft auf meiner RTX 3060 mit rund 77× Echtzeit, Whisper turbo mit 19×, large-v3 mit 10×.
> Bei der Genauigkeit sehe ich keinen Vorsprung: Normalisiert liegen Parakeet (7,7 %) und Whisper (6,8 % turbo, 7,7 % large-v3 auf den deutschen Sätzen) gleichauf, roh ebenfalls
> (15 bis 16 %), denn Parakeet v3 gibt Zeichensetzung und Großschreibung selbst aus. Mit 221 Wörtern ist die Messung zu klein für ein Ranking; für ein belastbares Urteil bräuchte es mehr Sprecher und mehr Text.

## Dateien

`rohdaten/` (je Lauf JSON mit Text und Latenz je Clip, `.meta` mit Last, VRAM und Exit-Code, `scores3.md` alle Tabellen), `skripte/` (`run2.sh` Ablauf unter dem
Host-Lock, `measure.sh`, `pk_bench.py`, `bench-whisper-main.rs` (Prüfstand außerhalb des Repos, Modellwahl über `BENCH_MODEL`), `score3.py`, `bootstrap.py`).
Modelle und venv liegen außerhalb des Repos (`/home/data/stt-test/models/`, `parakeet-venv`): Parakeet TDT 0.6B v3 ONNX `istupakov/parakeet-tdt-0.6b-v3-onnx` mit onnx-asr 0.12.0 /
onnxruntime-gpu 1.32.0 (CUDA-Provider, cuDNN 9, `cudnn_conv_algo_search = HEURISTIC`), Whisper `ggml-large-v3-turbo-q5_0.bin`, `ggml-large-v3-turbo.bin`, `ggml-large-v3.bin`
über whisper.cpp 1.8.3 (dictates Engine, whisper-rs 0.16). Alle Prozesse sind beendet, die GPU ist frei.
