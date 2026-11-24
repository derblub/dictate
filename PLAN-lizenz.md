# Plan: Lizenz und Veröffentlichung von dictate

Stand 23.11.2026. Grundlage: eine private Recherche vom 22.11.2026 (nicht im Repo), dazu eigene Prüfungen im
Repository (unten mit **gemessen** gekennzeichnet). Geschäftliche Teile (Abschnitt 3, Entscheidungen Nr. 5–8 und 12,
Punkte unter „Später“) stehen seit Block H2 privat, nicht im Repo; hier bleibt die Lizenztechnik. **Kein Rechtsrat:**
Zitate aus Lizenztexten und der FAQ der FSF belegen, was dort steht; ob es im Einzelfall so gilt, ist mit einem Anwalt
zu prüfen. Seit U1 (23.11.2026) stehen LICENSE, `license`-Felder und Lizenzköpfe im Repository (Abschnitt 6);
Abschnitt 1 beschreibt den Stand davor.
Jede Umsetzung ist ein eigener Schritt nach Daniels Entscheidung (Abschnitt 5).

Kennzeichnung je Befund: **gemessen** (im Repository oder an den Binaries geprüft), **belegt** (Quelle zitiert),
**Schätzung** oder **Annahme** (eigener Schluss), **offen**.

## 1. Befunde im Repository (23.11.2026)

| Was | Befund | Art |
|---|---|---|
| Lizenz | keine LICENSE, kein `license`-Feld in `Cargo.toml`; README nennt unter „Lizenzen und Quellen“ nur die EFF-Wortliste | gemessen |
| Autoren | alle 344 Commits von einem Autor (Daniel) | gemessen (`git log`) |
| niri-ipc | `=26.4.0`, `GPL-3.0-or-later` (Feld `license` im Crate); Abhängigkeit nur über das Feature `ui` | gemessen |
| Verwendung im Code | nur `src/bin/dictate-ui/niri.rs` (Typen `Request`, `Response`, `Reply`, `Action` mit 16 Varianten, `Window`, `Workspace`, `Outputs`, `SOCKET_PATH_ENV`) und `pal_host.rs` (`Vec<niri_ipc::Window>`); in `src/ui/` und `src/palette/` nur in Kommentaren. Der Socket wird schon selbst bedient (`UnixStream`, eine Zeile JSON hin, eine zurück, eigene Fristen); aus dem Crate kommen nur die serde-Typen | gemessen |
| Was die Binaries linken | `dictate` (Daemon): libstdc++, **libcublas.so.13, libcudart.so.13, libcuda.so.1**, libpulse(-simple), libgcc_s, libm, libc. `dictate-ui`: gtk4-layer-shell, GTK 4, Pango, Cairo, Graphene, GIO/GObject/GLib, libgcc_s, libm, libc (kein CUDA, kein Whisper). `dictate-settings`: nur libgcc_s, libm, libc | gemessen (`readelf -d target/release/*`) |
| niri-ipc im Binary | Zeichenketten `niri_ipc`/`niri-ipc`: `dictate-ui` 66, `dictate` 0, `dictate-settings` 0 | gemessen (`strings`, Build vom 23.11.) |
| Rust-Abhängigkeiten | 458 Pakete (über alle Plattformen samt Build-Abhängigkeiten, also eher zu viele): einziges GPL-Paket ist niri-ipc. Sonst MIT/Apache-2.0 (Mehrheit), Unicode-3.0 (ICU-Pakete), Zlib, ISC (ring, rustls-webpki, libloading), BSD-2/3-Clause, BSL-1.0 (clipboard-win, nur Windows), MPL-2.0 (option-ext), Unlicense (whisper-rs, whisper-rs-sys), CDLA-Permissive-2.0 (webpki-roots), `(MIT OR Apache-2.0) AND OFL-1.1 AND Ubuntu-font-1.0` (epaint_default_fonts), `Apache-2.0 OR GPL-2.0-only` (self_cell, Apache wählbar), `MIT OR Apache-2.0 OR LGPL-2.1-or-later` (r-efi, nur UEFI) | gemessen (`cargo metadata --offline`, nur lesend) |
| whisper.cpp | als Quelltext in whisper-rs-sys gebündelt (MIT laut privater Recherche) | belegt in der privaten Recherche; in U2 am Quelltext bestätigt (`LICENSE` von whisper.cpp in `THIRD-PARTY-LICENSES.txt`) |
| EFF-Wortliste | per `include_str!` in die Bibliothek kompiliert (`src/actions/wordlist.rs`), also im Daemon; CC BY, Hinweis in README und `assets/wordlists/README.md` | gemessen |
| Schriften | `egui`/`epaint` mit Feature `default_fonts`: OFL-1.1- und Ubuntu-Font-Schriften stecken in `dictate-settings` | gemessen (Features aus `cargo metadata`) |

**Korrektur aus U2** (23.11.2026, gelesen in `epaint` 0.37.0 `src/text/font_definitions.rs` und
`epaint_default_fonts` 0.37.0 `src/lib.rs`, `fonts/`; am alten Binary `~/.cargo/bin/dictate-settings` mit `strings`
gegengeprüft: „Ubuntu-Light“, „egui-icons“, „Bitstream“, „Source Foundry“, „Dalton Maag“ vorhanden):
Eingebettet waren Hack (MIT und Bitstream-Vera-Lizenz), Ubuntu Light (Ubuntu Font Licence 1.0) und `egui-icons` (Teilsatz
von emoji-icon-font, MIT). **Keine OFL-Schrift:** `OFL.txt` gehört zu Noto Emoji, das nur mit dem Feature
`monochrome_emoji_fonts` kommt (aus). Die Bitstream-Vera-Lizenz fehlte im SPDX-Feld des Crates. Die Zählung der Pakete
oben (458) lief über alle Plattformen; für das Linux-Ziel sind es 305 im normalen Graphen (Abschnitt 6, U2). Seit U2
ist `default_fonts` aus, es steckt keine dieser Schriften mehr im Binary.

## 2. Rechtliche Belege (Lizenztexte und FSF, abgerufen 23.11.2026)

- **Verkaufen ist erlaubt.** GPL-FAQ `#DoesTheGPLAllowMoney`: „Yes, the GPL allows everyone to do this.“ und „Except
  in one special situation, there is no limit on what price you can charge.“ (Die Ausnahme betrifft das schriftliche
  Angebot von Quellcode bei reiner Binärweitergabe.)
- **Quelltext für Empfänger.** GPLv3 Abschnitt 6(d): bei Weitergabe über einen Download-Ort „offer equivalent access to
  the Corresponding Source in the same way through the same place at no further charge.“
- **Linken bindet das ganze Werk.** GPL-FAQ `#GPLStaticVsDynamic`: „the terms and conditions of the GNU General Public
  License cover the whole combination.“ Daraus (Annahme, mit Anwalt prüfen): `dictate-ui` ist mit niri-ipc darin als
  Ganzes nur unter GPL-3.0 (oder später) weitergebbar.
- **Getrennte Programme über Sockets.** GPL-FAQ `#MereAggregation`: „pipes, sockets and command-line arguments are
  communication mechanisms normally used between two separate programs.“ Einschränkung dort: „But if the semantics of
  the communication are intimate enough“, kann es doch ein Programm sein. Daemon und `dictate-ui` sprechen über einen
  eigenen Unix-Socket; ob die Kopplung „intimate“ ist, bleibt offen (mit Anwalt prüfen).
- **Alleinurheber kann mehrfach lizenzieren.** GPL-FAQ `#ReleaseUnderGPLAndNF`: „you can release it under various
  different non-exclusive licenses at various times.“ Aber `#CanDeveloperThirdParty`: „the public already has the
  right to use the program under the GPL, and this right cannot be withdrawn.“ Folgerung (Annahme): Eine einmal
  veröffentlichte GPL-Fassung bleibt GPL; spätere Fassungen des **eigenen** Codes kann Daniel anders lizenzieren,
  solange er alleiniger Urheber ist (Beiträge Dritter nur mit CLA oder Abtretung, Entscheidung 11). Für `dictate-ui`
  gilt das nur, solange niri-ipc nicht mehr gelinkt ist.
- **Nicht GPL-kompatible Bibliothek (CUDA).** GPL-FAQ `#GPLIncompatibleLibs`: Wer ein GPL-Programm mit einer solchen
  Bibliothek linken will, „you need to provide permission to do that“, als Zusatzerlaubnis nach GPLv3 Abschnitt 7 mit
  dem Mustertext „Additional permission under GNU GPL version 3 section 7 — If you modify this Program, or any covered
  work, by linking or combining it with [name of library] […], the licensors of this Program grant you additional
  permission to convey the resulting work.“ Die „System Libraries“ (GPLv3 Abschnitt 1) decken nur Teile einer „major
  essential component (kernel, window system, and so on)“; ob libcudart und libcublas dazu zählen, ist zweifelhaft
  (Annahme), darum die ausdrückliche Erlaubnis. Erteilen kann sie nur, wer alle Urheberrechte am Binary hält: beim
  Daemon Daniel (kein fremder GPL-Code darin, gemessen), bei `dictate-ui` nicht (niri-ipc), dort ist aber auch kein CUDA.
- **Kompatibilität der übrigen Lizenzen mit GPLv3** (FSF-Lizenzliste): Apache-2.0 „compatible with version 3 of the
  GNU GPL“, Boost „compatible with the GNU GPL“, ISC und Zlib kompatibel, Unlicense kompatibel, MPL-2.0 über ihren
  Abschnitt 3.3 (MPL-Dateien bleiben MPL).
- **Nachtrag U2 (23.11.2026): die vor U5 offenen Lizenzen.** Quelle: FSF-Lizenzliste `license-list.html`. gnu.org war
  von blub nicht erreichbar (curl: Timeout über IPv4 und IPv6; WebFetch scheiterte mit Fehler), darum die Kopie des Internet Archive vom
  18.11.2026 (`web.archive.org/web/20261118023556/https://www.gnu.org/licenses/license-list.html`, Seitenstand
  „$Date: 2026/04/22“), abgerufen am 23.11.2026; Lizenztexte aus den Crates im Cargo-Registry-Cache.
  - **CC BY 4.0: belegt** (`#ccby`, „Licenses for Other Works“): „This is a non-copyleft free license that is good for
    art and entertainment works, and educational works. It is compatible with all versions of the GNU GPL; however,
    like all CC licenses, it should not be used on software.“ **Offen bleibt die Fassung:** eff.org nennt für die
    Seite 4.0 International und verlinkt 3.0 US (`assets/wordlists/README.md`); zu CC BY 3.0 hat die Liste keinen
    Eintrag. Die Namensnennung in `THIRD-PARTY-LICENSES.txt` nennt beide Fassungen (Annahme: erfüllt beide).
    **Geklärt in O1 (23.11.2026, U3d):** CC BY 4.0 festgelegt; die Verlinkung von 3.0 US ist nur das Badge, 2016 galt
    3.0 US.
  - **Unicode-3.0: belegt** (`#Unicodev3`, unter den GPL-kompatiblen): „It is a lax, permissive, free software
    license, compatible with all versions of the GNU GPL.“
  - **SIL OFL 1.1: entfällt (gemessen)**, keine OFL-Schrift im Binary, auch vor U2 nicht (Korrektur in Abschnitt 1).
    Zur Einordnung (`#SILOFL`, „Licenses for Fonts“): „The Open Font License (including its original release, version
    1.0) is a free copyleft license for fonts.“ Zur Vereinbarkeit mit der GPL sagt der Eintrag nichts.
  - **Ubuntu Font Licence 1.0: entfällt (gemessen)**, seit `default_fonts` aus ist (U2, Abschnitt 6); vorher im
    Binary (Ubuntu Light). Nicht in der FSF-Liste (Suche nach „Ubuntu“ ohne Treffer). Ihre Bedingung 4 („The Font
    Software, modified or unmodified, in part or in whole, must be distributed entirely under this licence, and must
    not be distributed under any other licence.“) wäre neben einem GPL-Binary eine Anwaltsfrage gewesen.
  - **Bitstream-Vera-Lizenz (in Hack): entfällt (gemessen)**, ebenfalls mit `default_fonts`; nicht in der FSF-Liste.
  - **CDLA-Permissive-2.0 (webpki-roots, die Wurzelzertifikate von Mozilla als Daten): offen**, nicht in der
    FSF-Liste (Suche nach „CDLA“ und „Community Data“ ohne Treffer). Wortlaut 2.1: „A Data Recipient may share Data,
    with or without modifications, so long as the Data Recipient makes available the text of this agreement with the
    shared Data.“; 3.1: „This agreement does not impose any restriction or obligations with respect to the use,
    modification, or sharing of Results.“ **Annahme:** die einzige Bedingung ist das Mitgeben des Textes, eine
    Hinweispflicht wie sie GPLv3 Abschnitt 7(b) zulässt; erfüllt durch `THIRD-PARTY-LICENSES.txt`. Vor U5 vom Anwalt
    bestätigen lassen. **O2 (23.11.2026, U3d):** genügt für U5 (Daniel), Anwaltsfrage offen; die CCADB verlangt
    zusätzlich ihre Nennung, ergänzt.
  - **LGPL-2.1 (neu gefunden): belegt.** `wayland-protocols-plasma` (Crate MIT) bündelt Protokolldateien von KDE unter
    LGPL-2.1 (`plasma-wayland-protocols/COPYING.LIB`). `#LGPLv2.1`: „It is compatible with GPLv2 and GPLv3.“
  - Aus derselben Kopie nachgeprüft: Expat/MIT „compatible with the GNU GPL“ (`#Expat`), Apache-2.0 „compatible with
    version 3 of the GNU GPL“ (`#apache2`); ISC, Zlib, Modified BSD, FreeBSD, Unlicense, Boost wie oben.
- **CUDA-EULA** (aus der privaten Recherche, nicht neu geprüft): Abschnitt 1.2 verbietet, das SDK einer Copyleft-Lizenz zu
  unterwerfen; 1.1.6 erlaubt Anwendungen unter OSI-Lizenzen; Mitliefern nur der Dateien aus Attachment A, unverändert.
  Am sichersten: keine NVIDIA-Bibliotheken mitliefern (Entscheidung 9).

## 3. (geschäftlich: privat, nicht im Repo)

Seit Block H2 privat. Die lizenztechnischen Folgen stehen an anderer Stelle: Quelltext am selben Ort wie jedes Binary
(GPLv3 6(d), Abschnitt 2 und Nr. 13), Zusatzerlaubnis für CUDA (Abschnitt 2, U1), Drittlizenz-Hinweise und Schriften
(U2), keine NVIDIA-Dateien mitliefern (Nr. 9).

## 4. Ginge die niri-IPC ohne die Bibliothek? (Entscheidung 2)

Ja, technisch ohne Weiteres (**abgeleitet**): niri dokumentiert das Protokoll selbst („if you're using a different
language, you are encouraged to communicate with the socket manually“, Kopf von `niri-ipc/src/lib.rs` 26.4.0:
`$NIRI_SOCKET`, eine Zeile JSON je Anfrage, eine Zeile `Reply` je Antwort). `niri.rs` macht den Socket-Teil heute schon
selbst; zu ersetzen wären nur die serde-Typen.

- **Umfang (Schätzung):** ein Modul mit eigenen Typen für 5 Anfragen, 5 Antworten, `Reply` (Ok/Err), 16 Aktionen und
  die gebrauchten Felder von `Window` (id, title, app_id, pid, workspace_id, is_floating, layout, focus_timestamp),
  `Workspace` und `Output` (logical.height); etwa 250–400 Zeilen samt Tests, 0,5 bis 1,5 Arbeitstage einschließlich
  Prüfschritt `wayland`.
- **Risiken:**
  - **Form auf der Leitung:** serde-Enums von niri sind extern getaggt; Einheitsvarianten (`"FocusedWindow"`) und
    leere Strukturvarianten (`{"FocusWorkspacePrevious":{}}`) unterscheiden sich, `Reply` ist `{"Ok":…}`/`{"Err":…}`.
    Abhilfe: vor dem Entfernen das heutige JSON jeder gebrauchten Anfrage aus niri-ipc als Fixture einfrieren und die
    eigenen Typen dagegen testen; dazu der Schritt `wayland` gegen das echte (verschachtelte) niri.
  - **Quelltext-Tests:** `no_shell_anywhere` (`src/bin/dictate-ui/primary.rs`) erwartet genau ein `Action::Spawn` in
    `niri.rs`, `sources_stay_apart` braucht jedes neue Modul in `MODULES`.
  - **Pflege:** ohne Crate fehlt die Hilfe des Compilers, wenn niri Felder umbenennt; dafür muss die Version nicht mehr
    exakt gepinnt werden (unbekannte Felder werden ignoriert). Ein Wechsel von niri bleibt ein Fall für `wayland`.
  - **Urheberrecht:** Feld- und Variantennamen sind die Schnittstelle; Typen aus der Protokollbeschreibung und dem
    beobachteten JSON schreiben, keine Doku-Kommentare oder Strukturen des Crates abschreiben. Die private Recherche nennt EuGH
    C-406/10 (SAS Institute) nach Sekundärquellen (mit Anwalt prüfen).
- **Nutzen:** nur, wenn `dictate-ui` (oder ein Binary, das UI-Code enthält) je unter einer anderen Lizenz als GPL
  stehen soll. Bei GPL für alles (Weg A) bringt der Ersatz rechtlich nichts.

## 5. Entscheidungen (Daniel, 23.11.2026)

13 offene Entscheidungen aus der privaten Recherche, einzeln vorgelegt, Lizenz zuerst, dazu eine Zusatzfrage (Nr. 14).
Korrektur zur Frage 1: Dort hieß es, alle übrigen 457 Rust-Pakete seien permissiv; genau genommen ist option-ext
MPL-2.0 (schwaches Copyleft je Datei, mit GPLv3 kompatibel) und epaint_default_fonts enthält OFL- und Ubuntu-Schriften
(Abschnitt 1). An der Entscheidung ändert das nichts. Alle am 23.11.2026 entschieden. Nr. 5–8 und 12 sind geschäftlich
und stehen seit Block H2 privat, nicht im Repo; die Nummern bleiben, damit Verweise stimmen.

| Nr. | Frage | Entscheidung | Ausgelöst wann |
|---|---|---|---|
| 1 | Lizenz | **GPL-3.0-or-later für das ganze Repository**, dazu eine Zusatzerlaubnis nach GPLv3 Abschnitt 7 für das Linken des Daemons mit NVIDIA CUDA (Mustertext der FSF, Abschnitt 2) | jetzt (Schritt U1) |
| 2 | niri-ipc ersetzen | **Behalten.** Unter GPL für alles kein rechtlicher Gewinn; die Trennung (niri nur in `dictate-ui`) bleibt erhalten, ein eigener Client (Abschnitt 4) ist jederzeit nachholbar | nur falls das UI je anders lizenziert werden soll |
| 3 | Reichweite | **Erst niri und CUDA**, Hyprland, sway und ein Vulkan-Weg werden vor jedem bezahlten Build eingeplant (Aufwand noch nicht geschätzt) | vor bezahlten Builds |
| 4 | Positionierung | **„Live und lokal, Endtext in Whisper-Qualität, landet unter Wayland zuverlässig im Feld, dazu Textaktionen, Palette und Diagnose.“** Barrierefreiheit ehrlich erwähnen (Freihandmodus), nicht als Zielgruppe bewerben, kein Ersatz für Sprachsteuerung | Release-Text |
| 5 | (geschäftlich) | privat, nicht im Repo | — |
| 6 | (geschäftlich) | privat, nicht im Repo | — |
| 7 | (geschäftlich) | privat, nicht im Repo | — |
| 8 | (geschäftlich) | privat, nicht im Repo | — |
| 9 | CUDA-Auslieferung | **Nie mitliefern**, CUDA kommt vom System der Nutzer | jetzt (Builds, AUR) |
| 10 | KI-Zugang | **Nur eigener Schlüssel (BYOK), frei; kein eigener Proxy.** Anthropics linked billing erst prüfen, wenn Nutzer ohne Schlüssel danach fragen | bei Nachfrage |
| 11 | Beiträge Dritter | **Code-Beiträge nur mit CLA**, die Umlizenzieren erlaubt; Text vor dem ersten Beitrag mit einem Anwalt klären (ebenso den Schutz KI-gestützt geschriebenen Codes). **Geändert am 23.11.2026:** Text selbst entworfen (U4), Fassung 1.0 ohne Durchsicht durch einen Anwalt (G6) | vor dem ersten fremden Beitrag |
| 12 | (geschäftlich) | privat, nicht im Repo | — |
| 13 | Vertriebskanäle | **Eigene Download-Seite (Quelltext und Builds am selben Ort, 6(d)) und AUR-Paket aus dem Quelltext** mit CUDA vom System; Flathub erst nach einem Test der Sandbox-Rechte | Release |
| 14 | Öffentliches Repository (Zusatzfrage) | **GitHub** (Issues, Releases, Sponsors); ~~das private GitLab bleibt Arbeitsort und spiegelt `main` dorthin~~. **Geändert am 23.11.2026 (U5, V1):** GitHub ist direkt der Arbeitsort, kein Spiegel; das GitLab-Projekt bleibt nur als privates Archiv des alten Verlaufs | Release |

## 6. Umsetzung (je ein eigener Schritt, Merge und Push nur mit Daniels Zustimmung)

- **U1 Lizenz ins Repository: erledigt am 23.11.2026** (Commits `3f90217`, `872f2a5`, `7f1570b` auf Branch `lizenz`).
  - `LICENSE`: `gpl-3.0.txt` von gnu.org, unverändert (SHA-256 `3972dc97…`, gleich `/usr/share/automake-1.20/COPYING`).
    Die lokale SPDX-Kopie `/usr/share/licenses/spdx/GPL-3.0-or-later.txt` ist im Wortlaut gleich, aber umformatiert
    (ohne Einrückung, typografische Anführungszeichen, ©), darum nicht genommen.
  - `license = "GPL-3.0-or-later"` in `Cargo.toml`, `tests/wayland/nested/Cargo.toml` und den vier
    `spikes/*/Cargo.toml` (über den Auftrag hinaus, folgt aus „das ganze Repository“).
  - **Kopf jeder `.rs`-Datei** (176 und der neue Test): Mustertext der GPL-FAQ `#GPLIncompatibleLibs`, wörtlich von
    gnu.org (23.11.), nur die Klammerstellen ersetzt, Satz in geschweiften Klammern entfernt. Wortlaut von Daniel am
    23.11. bestätigt; byte-genau als `RS_HEADER` in `tests/license_headers.rs` (einzige Quelle zum Kopieren für neue
    Dateien). Festgelegt dabei:
    - Copyright-Zeile wie im Muster („Copyright (C) 2026 Daniel Kurdoghlian“), darunter auf Daniels Wunsch eine eigene
      Zeile „Pushing Pixels, Vienna <https://pushingpixels.at>“ (Rechteinhaber bleibt Daniel persönlich).
    - „[name of library]“ = „NVIDIA CUDA (the libraries of the NVIDIA CUDA Toolkit, such as libcudart, libcublas and
      libcublasLt, and the CUDA driver library libcuda of the NVIDIA driver)“; Produktname statt nur Sonames, weil
      libcublas zusätzlich libcublasLt lädt (**gemessen**, `readelf -d`). „(or a modified version of that library)“
      bleibt wörtlich.
    - „[name of library's license]“ = zwei Namen: „NVIDIA CUDA Toolkit End User License Agreement“ (libcudart,
      libcublas: Paket `cuda` 13.5.0, `/opt/cuda/EULA.txt`) **oder** „NVIDIA Driver License Agreement“ (libcuda.so.1:
      Paket `nvidia-utils` 620.73.04, **gemessen** mit `pacman -Qo`). Der Plan nannte nur den ersten.
  - **Kurzkopf** (Daniels Wahl): 24 `.sh`, 4 `.py` und `tests/wayland/fake-logind` tragen nach dem Shebang
    Copyright, Pushing-Pixels-Zeile und `SPDX-License-Identifier: GPL-3.0-or-later`. Kein Kopf in KDL (ein Kopf in
    `niri/dictate-binds.kdl` wiche von der chezmoi-Kopie ab), Markdown, TOML, JSON, TSV, `.tmpl`, SVG; diese erfasst
    `LICENSE`/README.
  - **Test** `tests/license_headers.rs` (läuft im Schritt `unit`): durchläuft das Arbeitsverzeichnis ohne `target/`
    und versteckte Ordner (Spikes, Hilfs-Crate, auch unversionierte Dateien), Skripte auch am Shebang `#!/`;
    Mindestzahl und bekannte Pfade gegen einen leeren Durchlauf. Vor dem Einfügen rot (177 `.rs`, 29 Skripte).
  - README „Lizenzen und Quellen“: GPL-3.0-or-later für das ganze Repository samt Logo, CUDA-Zusatzerlaubnis,
    EFF-Wortliste ausgenommen (CC BY).
  - **Prüfung** (23.11.): `scripts/check.sh --only fmt,clippy,unit` grün (fmt 4 s, clippy 25 s, unit 629 s, 1713
    Tests), `cargo test --release --bin dictate-ui` 362 ok (darin `sources_stay_apart`, `no_shell_anywhere`),
    `--bin dictate-settings` 193 ok; Skripte mit `bash -n` und `ast.parse` geprüft (nur Kommentare geändert, darum
    `wayland`/`gui` nicht gelaufen). Nebenbefund: `spikes/*` und `tests/wayland/nested` sind nicht im Schritt `fmt` und
    weichen schon vorher von `rustfmt` ab (nicht im Kopfbereich); nicht angefasst.
  - Merge nach `main` und Push: einzeln bei Daniel erfragen.
- **U2 Drittlizenz-Hinweise: erledigt am 23.11.2026** (Branch `lizenz-u2` von `main` `eacb1f9`; Commits `bd99b65`
  Schriften, `18bb582` Drittlizenzen, danach Plan und CLAUDE.md).
  - **`THIRD-PARTY-LICENSES.txt`** (englisch, wird mitgeliefert), erzeugt von `scripts/third-party-licenses.py` aus
    `cargo metadata --offline --locked` für `x86_64-unknown-linux-gnu` mit Vorgabe-Features: alle Pakete im normalen
    Graphen (ohne Dev- und Build-Abhängigkeiten; Proc-Macros samt ihren Abhängigkeiten bleiben drin, weil ihr
    erzeugter Code im Binary steht): **305 Pakete, 184 verschiedene Lizenztexte**. Je Paket jede mitgelieferte
    Lizenzdatei, auch verschachtelte (ring `third_party/fiat`, regex-syntax `LICENSE-UNICODE`, tracing-core
    `src/spin`, wayland-protocols `COPYING`, wayland-protocols-plasma `COPYING.LIB` mit LGPL-2.1, whisper.cpp
    `LICENSE`); Test-, Doku- und Beispielordner ausgenommen (petgraph `assets/images`, tiff `tests`). Bei reinen
    OR-Ausdrücken die gewählte Lizenz („Used under“, etwa self_cell: Apache-2.0 statt GPL-2.0-only). Keine
    Apache-NOTICE-Datei im Graphen (gemessen). Deterministisch (kein Datum, keine Pfade), `--check` vergleicht.
  - **Rust-Standardbibliothek** (statisch in jedem Binary, fehlt in `cargo metadata`): unveränderte Kopie von
    `share/doc/rust/COPYRIGHT-library.html` der Toolchain 1.95.0 als `licenses/rust-std-1.95.0-COPYRIGHT-library.html`.
  - **whisper.cpp 1.8.3 / ggml 0.9.5** (in whisper-rs-sys 0.15.0): MIT, dazu der YaRN-Hinweis („MIT licensed.
    Copyright (c) 2023 Jeffrey Quesnelle and Bowen Peng.“) in `ggml-cpu/ops.cpp` und `ggml-cuda/rope.cu`, beide
    kompiliert (**gemessen** an den Objektdateien im Build); llamafile/sgemm.cpp (Mozilla) und kleidiai (Arm) werden
    für x86_64 mit CUDA nicht gebaut. MIT-Text von YaRN aus dem genannten Repository.
  - **EFF-Wortliste:** Namensnennung mit beiden CC-BY-Fassungen (Abschnitt 2, Nachtrag U2).
  - **Pakete ohne Lizenzdatei im Crate** (15: accesskit, acht egui-Pakete, gl, gtk4-layer-shell(-sys), profiling,
    whisper-rs-sys, dazu niri-ipc unter dem Text von `LICENSE`): Dateien aus dem Upstream am Commit aus
    `.cargo_vcs_info.json` unter `licenses/upstream/` (Herkunft `SOURCES.md`; der egui-Commit `7c0b404…` liegt nicht
    auf GitHub, genommen der Tag `0.37.0`, byte-gleich mit dem Commit von eframe). Fehlt eine Datei, bricht das Skript ab.
  - **Schriften (Entscheidung Daniel, 23.11.2026: abschalten):** `default_fonts` aus `eframe` entfernt;
    `epaint_default_fonts` nur noch als Test-Abhängigkeit (echte Schriftbytes für die Tests). `theme::install` beginnt
    mit `FontDefinitions::empty()`; liefert `fc-match` keine Basis (Frist, kein fontconfig), sucht
    `fonts::fallback_face` verbreitete Sans-Dateien unter `/usr/share/fonts`, `/usr/local/share/fonts` und
    `~/.local/share/fonts` (Entscheidung 22 bleibt: das Fenster geht trotzdem auf); `theme::complete` hängt die
    Basis-Kette an Titel und Mono, ohne jede Schrift endet das Fenster mit einer Meldung statt leerer Kästchen (egui
    zeichnet ohne Schrift „Tofu“, kein panic, gelesen in epaint 0.37 `MissingGlyphPolicy`). Tests:
    `egui_brings_no_fonts_of_its_own`, `fallback_finds_the_preferred_file_without_fontconfig`,
    `fallback_stops_at_the_depth_limit`, `complete_fills_title_and_mono_from_the_base_chain`.
  - **Prüfschritt `licenses`** (Entscheidung Daniel, 23.11.2026: direkt nach `fmt`): `cargo deny --offline check
    licenses` gegen `deny.toml` (13 erlaubte Lizenzen, unbenutzte verboten, OFL-1.1 und Ubuntu-font-1.0 absichtlich
    nicht) und `scripts/third-party-licenses.py --check`; rund 1 s. Rotproben: „Zlib“ aus der Liste (rc 4), ein
    Byte in der Datei geändert (rc 1), `default_fonts` wieder an (rc 4, epaint_default_fonts abgelehnt); danach grün.
  - **Prüfung** (23.11.): `scripts/check.sh --only fmt,licenses,clippy,unit,gui` grün in 690 s (fmt 1 s, licenses
    1 s, clippy 18 s, unit 542 s mit 1717 Tests, gui 128 s), dazu `cargo test --release --bin dictate-settings` 197 ok
    (vorher 193, vier neue Tests). Keine Schrift von egui mehr im Binary (**gemessen**): `epaint` ohne Feature
    `default_fonts` in `cargo metadata`, `epaint_default_fonts` nicht mehr im normalen Graphen, `cargo deny` lehnt
    das Wiedereinschalten ab; `strings` (ASCII, UTF-16 LE und BE) am alten installierten Binary gegen den neuen
    Release-Build: „Ubuntu-Light“ 1 → 0, „egui-icons“ 1 → 0, „Bitstream“ 6 → 0, „Source Foundry“ 2 → 0, „Dalton
    Maag“ 1 → 0 („hack“ trifft in beiden nur einen Shader-Kommentar von glow). Screenshots des Schritts `gui`
    (AI, Voice commands) ohne Kästchen. **Rückfall Ende zu Ende:** `tests/gui/shots.sh` mit einem `fc-match` vorn
    im PATH, das nur die Aufrufe von `dictate-settings` scheitern lässt: alle 14 Seiten öffnen, Log „base
    AdwaitaSans-Regular.ttf, title none“ (Titel dann in Gewicht 400), Screenshot ohne Kästchen; danach nichts übrig
    in `wl-run`. Abweichung: Budget (60 min) deutlich überschritten.
- **U3 Vor der Veröffentlichung: erledigt am 23.11.2026** (Branch `lizenz-u3` von `main` `792a764`; Commits `7442c05`
  Entscheidungen, `480d815` Fundliste, `e2bdb6c` Fixture, danach README und Plan; Prüfung: reine Doku bis auf die
  Fixture, dafür `scripts/check.sh --only fmt` und `cargo test --release --lib palette::sources::projects` grün, 9 Tests).
  Git-Verlauf und Dateien auf Geheimnisse und private Angaben prüfen (Pfade auf
  blub, Namen, `CLAUDE.md`, Pläne, Spikes, Testdaten). **Entschieden (Daniel, 23.11.2026):**
  - **Ganzer Verlauf wird öffentlich** (empfohlen war ein bereinigter Start-Commit; **ersetzt durch H1 vom selben
    Tag, siehe U3e: öffentlich wird nur ein neuer Start-Commit**). Folge: jeder Commit ist zu prüfen
    (`git log -p --all`, auch gelöschte Dateien und alte Fassungen), nicht nur der heutige Baum; ein Fund heißt
    Verlauf umschreiben (etwa `git filter-repo`) vor dem ersten Push nach GitHub, mit Folgen für den Spiegel von
    GitLab (Nr. 14) und nur mit Daniels Zustimmung je Fund.
  - **Öffentliche Doku englisch, intern deutsch** (wie empfohlen): README, CONTRIBUTING und Release-Texte englisch;
    Pläne, `CLAUDE.md`, Kommentare und Commits bleiben deutsch (Gedächtnis `dictate-sprache-deutsch`).
  - **Regel für diese Liste:** Sie geht selbst mit dem Verlauf an die Öffentlichkeit. Darum stehen hier nur Commit,
    Pfad, Art und Vorschlag, nie ein Geheimnis, eine echte Kennung von blub oder ein fremder Projekt- oder Kundenname;
    die Namen bekommt Daniel nur in der Rückfrage.

**U3a Prüfung des ganzen Verlaufs (23.11.2026, gemessen).**

- **Umfang:** alle Refs (`main`, `lizenz`, `lizenz-u2`, `lizenz-u3`, `origin/main`; keine Tags, kein Stash, keine
  sonstigen Refs): 360 Commits, ein Autor. Jeder Blob, der je im Verlauf stand (`git rev-list --all --objects`): 2025
  verschiedene, davon 10 binär (nur PNG), 380 Pfade, davon 92 heute gelöscht (u. a. `awesome/`, `design/pushingpixels/`,
  `docs/w5-vorlagen/`, `assets/brand/`, alte Binaries-Quellen). Dazu alle Commit-Nachrichten. Keine Audiodatei war je im
  Verlauf (Aufnahmen liegen nur unter `/home/data`).
- **Werkzeuge:** gitleaks (vorhanden, nichts installiert) über `git log --all` (347 Commits; die 13 Merges zeigt `git log -p` nicht), über den
  Arbeitsbaum und über jeden einzelnen Blob (damit auch Merges und gelöschte Dateien); eine eigene Suche mit 36 Mustern
  (Schlüssel und Tokens bekannter Anbieter, PEM, JWT, Zuweisungen, Pfade, Namen, E-Mail, IP, MAC, UUID, IBAN, Telefon,
  Adressen) über jeden Blob und alle Commit-Nachrichten; **exakter Abgleich** ohne Ausgabe der Werte: beide
  Anthropic-Schlüsseldateien sowie echte Kennungen von blub (machine-id, 22 Boot-IDs, 35 MACs, 22 IP-Adressen, die
  Tailscale-Adressen, -Namen und das Tailnet, GPU-UUID, Seriennummern der Platten, Namen der NetworkManager-Verbindungen)
  und 151 Ordnernamen unter `/home/git` (zwei Ebenen, dazu Repositories auf der dritten; Treffer nur die eigenen
  Projekte und der eine Name aus P2). Rohdaten unter `/home/data/stt-test/logs/u3/` (nicht im Repo,
  enthält Kopien aller Blobs; nach U5 löschen).

| Nr. | Art | Wo (Pfad, ab Commit) | Befund | Vorschlag |
|---|---|---|---|---|
| S1 | Geheimnisse | `src/actions/secrets.rs`, `src/palette/ai.rs`, `src/diagnose/ai.rs`, `src/diagnose/probes/text.rs`, `tests/palette_ai_http.rs` | alle Treffer von gitleaks (53 im Verlauf, 94 über alle Blobs) und der eigenen Muster sind Testmuster: Werte aus `rnd()`, das Beispiel `AKIA…EXAMPLE` aus der AWS-Doku, der allgemeine Kopf jedes OpenSSH-Schlüssels, `hunter2…`. **Kein echter Schlüssel**; der exakte Abgleich mit beiden Schlüsseldateien ergab 0 Treffer, auch in den Commit-Nachrichten | nichts |
| S2 | synthetische Logs | `tests/diagnose/corpus/*.txt` (ab `fcb058e`) | 46 Treffer von gitleaks (Kernel-Schlüssel-IDs, UUIDs, IDs). An allen vier Commits mit Änderungen (`fcb058e`, `259fb6f`, `181efaf`, heute) aus dem jeweiligen `generate.py` byte-gleich nachgebaut: rein synthetisch | nichts |
| S3 | Kennungen von blub | ganzer Verlauf | exakter Abgleich: keine machine-id, Boot-ID, MAC, Tailscale-Adresse oder -Name, GPU-UUID, Seriennummer. Treffer nur generisch (`127.0.0.1`, `172.17.0.1`, `docker0`, `tailscale0`) und der Rechnername (P4). Öffentliche IPs in Tests sind bekannte Resolver, Doku-Adressen oder ausgedachte (eine österreichische in `src/diagnose/pseudo.rs` ab `fcb058e`, gegen die Endpunkte von Tailscale geprüft: nicht die von blub) | nichts |
| S4 | Binärdateien | `docs/spikes/wayland-gtk/*.png` (5), Logos | Screenshots aus dem verschachtelten Test-niri mit Testtext; Metadaten nur Zeitstempel | nichts |
| P1 | Autor | alle 360 Commits (Autor und Committer) | voller Name und geschäftliche E-Mail-Adresse | Rückfrage F1 |
| P2 | fremde Projektnamen | `src/palette/sources/projects.rs` (Test, ab `abd6607`); `src/palette/resolve/tests.rs`, `src/diagnose/*`, `tests/diagnose/corpus/docker.txt`, Pläne | ein Ordnername aus `/home/git` mit möglichem Kundenbezug (10 Treffer, eine Datei); die eigenen Projekte (Data Room über 1000 Treffer, niri-coach, continuity, awesome-claude-usage) in Tests und Plänen | Rückfrage F2 |
| P3 | Marke und Studio | `design/pushingpixels/project/` (ab `39fbc93`, gelöscht in W6 Block E), `assets/brand/`, `src/gui/assets/*.png` | Auszüge des Pushing Pixels Design Guide mit Positionierung des Studios, Logos, in `tokens.json` der Host des privaten GitLab | Rückfrage F3 |
| P4 | Pfade, Rechnername | Pläne, `CLAUDE.md`, README, Tests, Vorgaben im Code, 2 Commit-Nachrichten | `/home/blub`, `/home/git`, `/home/data`, „blub“ | behalten (Entscheidung „ganzer Verlauf“); Vorgaben im Code verallgemeinert (O3, erledigt) |
| P5 | privater GitLab-Host | `CLAUDE.md`, `src/actions/secrets.rs` (Testfälle), `tokens.json` | Host und Pfad des Repos, keine Zugangsdaten | behalten |
| P6 | Arbeitsnotizen | `CLAUDE.md`, `PLAN-*.md`, `SPEC-v2.md`, `ROADMAP.md`, `docs/` | Arbeitsweise mit Agenten (Orca, Sitzungen, Worker anderer Projekte), Hardware, Einrichtung von blub (pstore-ACL, espanso, chezmoi, `sleep.conf`), Ort der Schlüsseldatei und dass ein Schlüssel ungültig war (kein Wert), geschäftliche Teile dieses Plans | ~~behalten (Entscheidung „ganzer Verlauf“)~~; **neu (H2, U3e), erledigt in Block H2:** Technik bleibt im öffentlichen Baum, die geschäftlichen Teile dieses Plans stehen privat, nicht im Repo, der Rechnername in `CLAUDE.md` ist neutral gefasst; weiter gilt: alles in diesen Dateien wird öffentlich |
| P7 | eigene Einrichtung im Produkt | `src/config.rs` (`DIAGNOSE_DEFAULT_TEMP_LOG_DIR`, `PALETTE_DEFAULT_ROOT`), `.cargo/config.toml` (nur `sm_86`, `/opt/cuda`), Palette (Widget Türkamera über ein eigenes Programm, Projekte in Alacritty mit Claude Code, temp-logger) | Vorgaben und Funktionen passen nur auf blub | kein Umschreiben; verallgemeinert (O3, erledigt; Noctalia und rofi in O6, erledigt) |
| P8 | Fremdmaterial | ganzer Verlauf | Lua-Module, Vorlagen und Design-Texte sind eigene; fremd nur EFF-Wortliste und `licenses/` (beide mit Lizenz) | nichts |

Kein Fund **erzwingt** ein Umschreiben. Für F1–F3 gilt dieselbe Rechnung: jeder Hash ab dem ersten betroffenen Commit
ändert sich (F1: alle 360), Hash-Verweise in Plänen und `CLAUDE.md` zeigen danach ins Leere (Commit-Nachrichten
schreibt `git filter-repo` mit um, Dateiinhalte nicht), `main` auf dem GitLab braucht einen Force-Push, offene
Branches einen Rebase. `git filter-repo` ist nicht installiert (Installation nur mit Daniels Zustimmung). Empfehlung:
jedes Ja in **einem** Durchgang direkt vor dem ersten Push nach GitHub (U5), nicht jetzt.

**U3b Entscheidungen (Daniel, 23.11.2026), jeweils wie empfohlen außer F4:**

| Nr. | Frage | Entscheidung | Umsetzung |
|---|---|---|---|
| F1 | E-Mail-Adresse in allen Commits ersetzen? | **Behalten** | nichts |
| F2 | fremde Projektnamen | **Nur den Namen mit möglichem Kundenbezug ersetzen**, eigene Projekte (Data Room usw.) bleiben | im Baum erledigt (`e2bdb6c`, neutraler Name in der Fixture); ~~im Verlauf ab `abd6607` mit `git filter-repo --replace-text` (O4)~~ entfällt mit H1 (der alte Verlauf wird nicht veröffentlicht) |
| F3 | Pushing Pixels Design Guide und Logos aus dem Verlauf entfernen? | **Behalten** | nichts |
| F4 | Anordnung der READMEs | **Nur Englisch** (empfohlen war: englisch als `README.md`, deutsch nach `docs/`): die deutsche README entfällt, gepflegt wird nur noch die englische; Einzelheiten stehen in den Plänen | erledigt, siehe U3c |

**U3c Englische README: erledigt.** `README.md` ist jetzt englisch, die deutsche ist entfernt (im Verlauf erhalten).
Aufbau nach Entscheidung 4: Positionierung, Funktionen, Barrierefreiheit ehrlich, Datenschutz, Voraussetzungen,
Installation aus dem Quelltext, Farben, Bedienung samt Ausgabe und KI-Nachbearbeitung, Textaktionen, Befehlspalette,
Linux-Diagnose, Konfiguration mit allen Tabellen der deutschen Fassung, Aufbau, Leistung, Entwicklung, Beiträge noch
nicht möglich, Lizenz. Alles für Nutzer Wichtige der deutschen Fassung ist übertragen; weggelassen sind Hinweise nur
für blub (Installation „blub“, die pstore-ACL von blub ist als allgemeiner Hinweis gefasst) und Verweise auf interne
Pläne. Ohne Links, die es noch nicht gibt (GitHub, AUR, Spenden). Jeder Befehl, Pfad, jede Vorgabe und Modellquelle
gegen das Repo geprüft (`niri-ipc` `repository`, `src/models.rs`, `src/config.rs`, `systemd/`,
`niri/dictate-binds.kdl`, `AI_PRESETS`); die Aussagen zu Datenschutz und Vorgaben gegen den Code (`AiCfg`/`FeaturesCfg`
in `src/config.rs`: KI und Verlauf aus, Vorgabe-Server lokales Ollama; Geheimnisfilter nur bei Textaktionen, Palette
und Diagnose, nicht beim Diktat; `ASK_FRAME` ohne `INPUT_GUARD`; Palette-Absicht `suspend`). Die Vorgaben `project_roots` und `temp_log_dir` stehen wahrheitsgemäß mit dem
Hinweis, dass sie allgemein werden (O3).

**U3d Offen vor U5:**

- O1 **Fassung von CC BY der EFF-Liste** (Abschnitt 2, Nachtrag U2): eff.org nennt 4.0 und verlinkt 3.0 US.
  **Erledigt am 23.11.2026** (Branch `lizenz-o1o2` von `main` `7375172`). Belege (alle abgerufen am 23.11.2026, nur
  lesend):
  - **Die Datei** `https://www.eff.org/files/2016/07/18/eff_large_wordlist.txt`: SHA-256 gleich der Kopie im Repo
    (gemessen), `Last-Modified: Mon, 18 Jul 2016 22:41:51 GMT`; die Datei trägt keinen Lizenzhinweis (alle 7776 Zeilen
    im Format `NNNNN<TAB>wort`, gemessen). Die Seiten `https://www.eff.org/dice` und der Blogbeitrag von 2016
    (`https://www.eff.org/deeplinks/2016/07/new-wordlists-random-passphrases`, Joseph Bonneau, 19.07.2016) nennen die
    Lizenz nur im Fuß: „Copyright (CC BY)“ mit Link auf `/copyright` (belegt).
  - **Herkunft der Wörter:** Der Blogbeitrag: „We based our list off of data collected by Ghent University's Center for
    Reading Research“, danach Auswahl, Filter und Handprüfung durch die EFF: „The result is our own list of 7,776
    words“ (belegt). **Annahme:** Die Liste ist damit eigenes Material der EFF im Sinn ihrer Copyright-Seite; aus den
    Daten aus Gent stehen nur einzelne Wörter darin, keine Werte (Bekanntheit, Konkretheit). Ob die Daten aus Gent
    eigene Bedingungen haben, ist nicht geprüft (offen, für O1 nicht nötig: die EFF gibt die Liste als ihre aus).
  - **Heute** `https://www.eff.org/copyright`: „Any and all original material on the EFF website may be freely
    distributed at will under the Creative Commons Attribution 4.0 International License (CC-BY), unless otherwise
    noted.“ Der Satz verlinkt `creativecommons.org/licenses/by/4.0/`; auf `by/3.0/us/` zeigt nur das Badge-Bild davor
    (belegt). Die Seite bittet: „Please do not write to us asking for permission“.
  - **Verlauf** (Internet Archive, `web.archive.org/web/<Zeitstempel>id_/https://www.eff.org/copyright`): Juli 2016
    (`20160701003037`, `20160812030235`) „under the Creative Commons Attribution License“ mit Link auf
    `creativecommons.org/licenses/by/3.0/us/` (Linkziel ausgelesen für 2012/13, Mai bis Dezember 2016 und
    `20250102181955`; die übrigen Stichproben 2017 bis `20250127035452` zeigen denselben Satz ohne 4.0); ab
    `20250305115656` der Text mit 4.0 (belegt). Der Wechsel liegt also zwischen 27.01. und 05.03.2025; die Kopie im Repo (abgerufen 23.10.2026) stammt
    aus der Zeit von 4.0.
  - **Unwiderruflich:** CC BY 4.0 Abschnitt 2(a)(1) „irrevocable license“; CC BY 3.0 US Abschnitt 7(b) „Licensor
    reserves the right to release the Work under different license terms […] provided, however that any such election
    will not serve to withdraw this License“; CC-FAQ (`creativecommons.org/faq/`): „The CC licenses are irrevocable.“
    (belegt). **Annahme:** Für die unveränderte Datei gelten damit beide Angebote; dictate stützt sich auf 4.0.
  - **Pflichtangaben 4.0** (Abschnitt 3(a)(1)): Urheber, Copyright-Vermerk, Hinweis auf Lizenz und
    Gewährleistungsausschluss jeweils „if it is supplied by the Licensor“, URI des Materials, Änderungen angeben, Lizenz
    nennen mit Text oder URI. Die EFF liefert weder Copyright-Vermerk noch Ausschluss mit; der Rest steht in
    `THIRD-PARTY-LICENSES.txt` Abschnitt 3 (gemessen).
  - **GPL:** Die FSF-Liste nennt CC BY 4.0 „compatible with all versions of the GNU GPL“ (Abschnitt 2); zu 3.0 US hat
    sie keinen Eintrag.

  **Entscheidung O1 (Daniel, 23.11.2026, wie empfohlen):** CC BY 4.0 International festlegen, mit einem Satz zur
  Fassung bei Veröffentlichung (2016: 3.0 US). Nicht gewählt: beide Fassungen nennen; die EFF fragen. Umsetzung in
  `f07d9e1`: `scripts/third-party-licenses.py` (Abschnitt 3, daraus `THIRD-PARTY-LICENSES.txt`),
  `assets/wordlists/README.md`, `README.md`, Kommentar in `src/actions/wordlist.rs` (nannte 3.0 US), `CLAUDE.md`.
- O2 **CDLA-Permissive-2.0** (webpki-roots) beim Anwalt bestätigen (Abschnitt 2).
  **Für U5 erledigt am 23.11.2026, Anwaltsfrage offen** (Branch `lizenz-o1o2`). Belege (abgerufen am 23.11.2026):
  - **Lizenztext** (`LICENSE` im Crate webpki-roots 1.0.10; die Sätze 2.1 und 3.1 wortgleich mit
    `https://cdla.dev/permissive-2-0/` verglichen, nicht der ganze Text): einzige
    Bedingung 2.1 „makes available the text of this agreement with the shared Data“; erfüllt durch
    `THIRD-PARTY-LICENSES.txt` Abschnitt 5 (gemessen).
  - **CDLA-FAQ** (`https://cdla.dev/faq-resources/faq/`): „If I receive Data under CDLA-Permissive-2.0, can I
    redistribute it under different terms? Yes – just as you can distribute software under a permissive license in a
    proprietary software product (subject to retention of required notices), you can redistribute the permissively
    licensed data in a larger aggregation under any terms you desire.“ Die Fassung 2.0 verlangt ausdrücklich keine
    Namensnennung (belegt). Das ist die Sicht des Herausgebers der Lizenz, nicht die der FSF.
  - **Herkunft der Daten:** README des Crates: „The underlying data is used via the CCADB Data Usage Terms […] The data
    in this crate is a derived work of the CCADB data.“ Die CCADB (`https://www.ccadb.org/rootstores/usage`, „CCADB
    Data Usage Terms“) nennt CDLA-Permissive-2.0 und unter „Key Terms“ zusätzlich: „Attribution to the Common CA
    Database (CCADB) is required when redistributing or using this data in derived works.“ und „The names and
    trademarks of the CCADB or any contributors may not be used to endorse or promote products derived from this data
    without prior written permission.“ (belegt). Ob diese Zusätze über den Lizenztext hinaus binden, ist **offen**.
    **Annahme:** Beide entsprächen Zusatzbedingungen, die GPLv3 Abschnitt 7(b) („preservation of specified reasonable
    legal notices or author attributions“) und 7(d)/(e) (Namen zu Werbezwecken, Marken) zulässt (Buchstaben gegen
    `LICENSE` geprüft).
  - **Listen:** SPDX `CDLA-Permissive-2.0` (`https://spdx.org/licenses/CDLA-Permissive-2.0.json`):
    `isOsiApproved: false`, keine FSF-Einstufung. Fedora (`fedora-licenses.json` aus `fedora-license-data`):
    Status „allowed“. FSF-Liste: kein Eintrag (Abschnitt 2). (belegt)
  - **Distributionen:** Debian hat `rust-webpki-roots` zweimal entfernt (`ftp-master.debian.org/removals-full.txt`):
    08.11.2020 „ROM; duplicates ca-certificates“ (Bug 972802), 09.01.2025 „ROM; hardcoded root certs are unfit for
    stable Debian“ (Bug 1069946), also aus Sicherheits- und Paketgründen, nicht wegen der Lizenz (belegt). webpki-roots
    steht erst seit 0.26.9 (27.04.2025) unter CDLA-Permissive-2.0, davor MPL-2.0 (crates.io, belegt). **Offen,
    versucht:** ein GPL-Programm in einer Distribution, das die CDLA-Fassung einbaut. Unter den ersten 500 der 1745
    direkten Abhängigen auf crates.io gibt es GPL-Crates; `sn0int` (Arch `extra`, GPL-3.0-or-later) baut aber nur webpki-roots 0.17–0.21
    (MPL) ein, `fractal` und `shortwave` (Arch, GPL-3.0) nutzen webpki-roots nicht. Der Paketquelltext von Fedora und
    die Bugs von Debian waren hinter einer Bot-Sperre nicht lesbar.
  - **Schätzung:** Wurzelzertifikate (Namen, öffentliche Schlüssel, Gültigkeit) sind vor allem Tatsachen; wie viel
    Schutz sie überhaupt genießen, ist offen und für die Entscheidung nicht nötig.

  **Entscheidungen O2 (Daniel, 23.11.2026, wie empfohlen):**

  | Nr. | Frage | Entscheidung |
  |---|---|---|
  | O2a | Anwalt vor U5? | **Nein, die Beleglage genügt für U5.** Die Anwaltsfrage (unten) bleibt offen und wird später geklärt. Nicht gewählt: Anwalt vor U5; Systemzertifikate statt webpki-roots (Projektregel „Produktion immer webpki-roots“ bleibt; Alternative wäre z. B. rustls-native-certs: Vertrauen hinge am Speicher des Systems samt dort eingetragener Wurzeln, neuer Code in `src/http.rs` und Tests, geschätzt 2–4 h) |
  | O2b | CCADB nennen? | **Ja:** `scripts/third-party-licenses.py` führt feste Hinweise je Paket (`NOTES`, bricht ab, wenn das Paket aus dem Graphen fällt); bei webpki-roots nennt `THIRD-PARTY-LICENSES.txt` die CCADB mit Link auf die Data Usage Terms (`f07d9e1`) |

  **Anwaltsfrage (offen, kein Termin):** „Ein unter GPL-3.0-or-later (mit Zusatzerlaubnis nach Abschnitt 7 für NVIDIA
  CUDA) veröffentlichtes Programm kompiliert die Wurzelzertifikate des Crates webpki-roots ein. Der Crate steht unter
  CDLA-Permissive-2.0; seine Daten stammen aus der CCADB, deren Data Usage Terms zusätzlich Namensnennung verlangen und
  die Werbung mit den Namen der CCADB untersagen. Wir geben den CDLA-Text und eine Nennung der CCADB mit. (1) Ist die
  Weitergabe des Gesamtwerks unter GPL-3.0-or-later damit zulässig, insbesondere mit GPLv3 Abschnitt 7 und 10? (2)
  Binden die Zusatzbedingungen der CCADB Empfänger des Crates, und genügt unsere Nennung? (3) Ändert sich etwas, wenn
  Dritte (Distributionen, AUR) das Programm aus dem Quelltext bauen?“
- O3 **Eigene Einrichtung im Produkt verallgemeinern** (P4, P7): Vorgaben ohne `/home/blub` und `/home/git`,
  `.cargo/config.toml` für andere GPUs und CUDA-Pfade, Türkamera und temp-logger als optionale Funktionen. Eigener
  Schritt mit Prüfung, kein Umschreiben.

  **O3 Entscheidungen (Daniel, 23.11.2026, alle wie empfohlen; Branch `lizenz-o3`):**

  | Nr. | Frage | Entscheidung |
  |---|---|---|
  | E1 | Vorgabe `palette.project_roots` | `["~"]` mit Tiefe 2 (Repos in `~/x` und `~/x/y` ohne Einrichtung) |
  | E2 | temp-logger | `diagnose.temp_log_dir` leer = aus (Vorgabe): `temps_tail` meldet „nicht eingerichtet“ und bleibt in den Profilen; leer ist erlaubt, „Reset“ setzt leer; README beschreibt das CSV-Format |
  | E3 | Widget Türkamera | aus dem Produkt entfernen; der Autor bekommt dasselbe über einen eigenen Desktop-Eintrag (`launch_app`), außerhalb des Repos |
  | E4 | Terminal | neues Feld `palette.terminal` (argv-Präfix); leer = automatisch (`xdg-terminal-exec`, sonst das erste installierte aus einer festen Tabelle); Arbeitsordner über `env -C`, keine Shell |
  | E5 | Befehl im Projektordner | neues Feld `palette.project_command` (argv); leer = nur die Shell des Terminals; Texte neutral |
  | E6 | Noctalia (Sperre, Zwischenablage) und rofi-emoji | nicht in O3; offener Punkt vor U5 (O6, erledigt: K1–K3) |

  Nicht als Frage vorgelegt (Build, keine Produktentscheidung): `.cargo/config.toml` ohne feste CUDA-Werte; whisper.cpp
  wählt mit `GGML_NATIVE` (Vorgabe ohne `SOURCE_DATE_EPOCH`) die Architektur der eingebauten GPU, nvcc kommt aus `PATH`
  bzw. `CUDA_PATH`. Wer Werte setzt, tut es über Umgebungsvariablen (`CMAKE_CUDA_ARCHITECTURES`, `CUDACXX`).

  **O3 erledigt (23.11.2026, Branch `lizenz-o3`):**
  - Build: `.cargo/config.toml` entfernt (`1ffc455`). Geprüft per CMake-Configure von whisper.cpp ohne die Variablen:
    `CMAKE_CUDA_ARCHITECTURES=86-real` (native), nvcc `/opt/cuda/bin/nvcc`; der Release-Build danach baute whisper-rs-sys
    nicht neu (`build.rs` beobachtet `CMAKE_*`/`CUDA*` nicht, README nennt `cargo clean -p whisper-rs-sys`).
  - E1/E2 (`a0e4003`): `PALETTE_DEFAULT_ROOT` = `~` (auch `palette::sources::projects::DEFAULT_ROOT`, aufgelöst);
    `DIAGNOSE_DEFAULT_TEMP_LOG_DIR` leer, `temp_log_path` gibt `Option`, `ProbePaths` trägt `Option`, `{temp_log_dir}/…`
    erlaubt ohne Ordner nichts, `temps_tail` meldet `Unavailable` „no temperature log is set up“; Knopf „Reset to
    default“ im Fenster entfällt (leer ist die Vorgabe).
  - E3–E5 (`fffa69a`, Befunde der Durchsicht `b268cef`): `Widget::Doorcam` samt Stichwort entfernt (Katalog lehnt
    `doorcam` ab); `palette.terminal` und `palette.project_command` (argv, `PaletteCfg::check_argv`: höchstens 32
    Einträge, nicht leer, ohne Steuerzeichen, `argv[0]` Name ohne Leerraum oder absoluter Pfad, nicht `-…`); der Daemon
    schickt sie mit `web_search_url` in jeder Antwort von `pal_resolve`/`pal_ask` (`palette::service::ExecPrefs`, Spiegel
    `ui::pal_exec::ExecPrefs`); automatisch `ui::pal_exec::TERMINALS` (xdg-terminal-exec, alacritty, kitty, foot,
    ghostty, wezterm, konsole, gnome-terminal, xterm, je mit Ordnerschalter laut Handbuch, dazu immer `env -C`); ohne
    Befehl die Shell (`SHELL`, sonst `sh` im Suchpfad, ohne Befehlstext); Texte neutral („Open project X in a
    terminal“); Fenster: Gruppe „Terminal“ (je Argument eine Zeile).
  - Durchsicht (Sonnet, nur lesend): übernommen „kitty -e“ in einem Eintrag, Ordnerschalter je Terminal (Terminals mit
    Server erben `env -C` nicht sicher), Tests für die Weitergabe in `cmdpal` und für `=`, Meldung nennt das Terminal
    statt env, GUI-Prüfung mit frischen leeren Zeilen, README zum Zeitformat. Ungeprüft: die Ordnerschalter von
    kitty, foot, ghostty, wezterm, konsole und gnome-terminal sind nur gegen die Handbücher geschrieben, auf blub ist nur
    alacritty installiert.
  - Prüfung (23.11.): `check.sh --only fmt,clippy,unit,gui,wayland` grün: `unit` 873 s auf `fffa69a`; nach den
    Befunden der Durchsicht gezielt `--lib ui:: config:: palette:: diagnose::probes` (595), `--bin dictate-ui` (365),
    `--bin dictate-settings` (197), `--test daemon_palette` (23), `--test daemon_diagnose` (19), dann `fmt,clippy,
    wayland,gui` auf `b268cef` (wayland 510 s mit P19 Projekt öffnen: Ordnerschalter, `env -C` geprüft über das
    Arbeitsverzeichnis des Fakes, Terminal und Befehl aus der Config; gui 112 s). Release-Build ohne
    `.cargo/config.toml` 3:14 min, nur dictate neu.
  - Für die Einrichtung des Autors nach der Installation (im Fenster, nicht vorher: die alten Binaries kennen die neuen
    Felder nicht, und Werte gleich der alten Vorgabe ließe das alte Fenster weg): Palette › Project roots `/home/git`,
    Command in projects `claude`, Terminal leer (wählt alacritty); Diagnose › Temperature log folder auf den Ordner des
    eigenen Loggers.
- O4 **Entfallen mit H1 (U3e, 23.11.2026):** der alte Verlauf bleibt privat, `git-filter-repo` wird nicht gebraucht.
  Ursprünglich: **F2 im Verlauf umsetzen**, direkt vor dem ersten Push nach GitHub: `git filter-repo --replace-text` für den
  einen Namen (braucht die Installation von `git-filter-repo`, nur mit Daniels Zustimmung). Folgen: Hashes ab
  `abd6607` ändern sich, Hash-Verweise in Plänen und `CLAUDE.md` danach veraltet, `main` auf dem GitLab per
  Force-Push, offene Branches neu aufsetzen. Danach diese Prüfung über den neuen Verlauf wiederholen (Skripte und
  Muster wie oben; der Name darf in keinem Blob mehr stehen).
- O5 Rohdaten unter `/home/data/stt-test/logs/u3/` nach U5 löschen.
- O7 **Erledigt am 23.11.2026** (Branch `lizenz-o7` von `main` `6c93012`; Commit `a2d626c`, danach Plan und
  `CLAUDE.md`). Umsetzung:
  - **Ein Auflöser je Sprache, eine Regel:** `scripts/test-root.sh` (`dictate_test_root`, eingebunden von `check.sh`,
    `record-testset.sh` und dem Harness) und `tests/test_root/mod.rs` (vier Integrationstests, `live_eval.rs` per
    `#[path]`). `DICTATE_TEST_ROOT`, sonst `${XDG_CACHE_HOME:-$HOME/.cache}/dictate-test` (relatives
    `XDG_CACHE_HOME` zählt nicht, ohne absolutes `HOME` Fehler). Erlaubt sind nur absolute Pfade ohne leere, `.`- und
    `..`-Teile aus `A-Z a-z 0-9 . _ - /`: der Pfad landet in `rm -rf` (Aufräumen alter Läufe), in Globs von `compgen`
    und in der Adresse des privaten D-Bus, die andere Zeichen maskiert. Einzelvariablen (`DICTATE_TESTSET_DIR`,
    `DICTATE_TEST_LOGS`, `LOGS`, `DICTATE_SPEECH_OUT`, `DICTATE_TESTSET_OUT`, `DICTATE_DIAG_OUT`) übersteuern weiter.
  - **Harness:** löst den Ordner beim Einbinden auf und exportiert ihn, bevor `nested_up` `XDG_CACHE_HOME` umbiegt;
    `WL_RUN_BASE=$DICTATE_TEST_ROOT/wl-run`, bei untauglichem Ordner leer, dann scheitert `nested_up`, und `run.sh`,
    `diag-shots.sh` brechen sofort ab; die Prüfungen `XDG_RUNTIME_DIR == "$WL_RUN_BASE"/*` verlangen zusätzlich ein
    nicht leeres `WL_RUN_BASE` (leer hieße `/*`). **Socket-Länge:** der längste Socket im Laufzeitordner ist
    `sway-ipc.<uid>.<pid>.sock` (bis 32 Zeichen), der Laufzeitordner `<root>/wl-run/n.XXXXXX/rt/`; darum höchstens 55
    Zeichen für den Root (genau dann 107), sonst klare Meldung (gemessen: 55 geht, 56 nicht). Vorgabe auf blub
    `/home/blub/.cache/dictate-test` hat 30.
  - **Wächter** von `nested-inject` und `nested-clip`: `tests/wayland/nested/src/guard.rs`, reine Funktion von Root,
    `XDG_RUNTIME_DIR` und `WAYLAND_DISPLAY`. Der Root muss gesetzt sein (keine Vorgabe im Binary), alle drei Werte
    sauber (Regel wie oben), der Laufzeitordner unter `<root>/wl-run/` (mit Schrägstrich: `wl-run-x` scheitert).
    Mindestens so streng wie vorher: weil kein `..` durchgeht, passt der Ordner der echten Sitzung (`/run/user/<uid>`)
    zu keinem Root, denn über ihm liegt kein Ordner `wl-run`. Fünf Tests je Binary (auch „echte Sitzung bei jedem
    Root abgelehnt“); `check.sh` führt sie in `need_nested` aus (vorher nur gebaut).
  - **Sperre und Logs** liegen im Ordner: zwei Ordner heißen zwei Sperren. `check.sh` legt den Ordner an und nennt ihn
    in der ersten Zeile, die Zeile „SKIPPED“ von `speech` nennt ihn ebenfalls (auf blub ohne Variable würde `speech`
    sonst unbemerkt übersprungen; `gpu` scheitert dann mit Pfad und Hinweis statt `unwrap`).
  - **Nicht umgestellt:** die Spikes unter `spikes/` (eigene feste Prüfung `/home/data/stt-test/wl-run/`, laufen nur
    mit `DICTATE_TEST_ROOT=/home/data/stt-test`; Archiv, Vermerk im Harness und in `CLAUDE.md`), historische
    Messorte in Kommentaren (`commands.rs`, `thresholds.tsv`, `ai_http.rs`) und die Fixture in `pseudo.rs`.
  - **Doku:** CONTRIBUTING („Test folder“ statt „Current limitation“, Bedarf von `speech`/`gpu` mit den Dateien, die
    nicht im Repo sind), README „Development“, `CLAUDE.md` (Absatz unter „Bauen und installieren“, Pfade).
  - **Prüfung:** ohne Variable (Vorgabe unter `~/.cache`) `fmt,clippy,gui` grün (144 s), danach den Ordner entfernt;
    die Länge an der Grenze und mit einem zu langen Root (Scratchpad, 116 Zeichen) ohne cargo geprüft. Mit
    `DICTATE_TEST_ROOT=/home/data/stt-test` die volle Suite: `fmt`, `licenses`, `clippy`, `unit`
    (417 s) grün, `wayland` scheiterte zweimal unter Last 17 (Tests eines anderen Projekts) an je einer anderen
    Zeitprüfung: zuerst `palette.sh` „grabbed fast“ (1111 ms statt < 1000) und „screenshot area: done in the log“ (Eintrag nicht
    binnen 3 s), dann
    `T3: route input method` (450 ms bis zur Entscheidung, Text dennoch exakt); keine davon hängt an Pfaden. Danach
    `gui` (118 s), `speech` (242 s, Gesamt-CER 0,0058, Schwelle 0,04) und `gpu` (102 s) grün, `wayland` im dritten
    Lauf grün (537 von 537, 548 s, Last um 11). Die Zeitprüfungen unter Last gehören zur Liste „Beobachten“
    (Wackeln von `palette.sh`). `live_eval` mit `DICTATE_LIVE_DIAG=dry` ohne Variable: Berichte unter
    `~/.cache/dictate-test/logs/diagnose-ai/<Zeit>/` (kein API-Aufruf), danach entfernt. Nur auf Syntax geprüft
    (`bash -n`): `record-testset.sh` (braucht ein Terminal), `diag-shots.sh`, `shots.sh`, `interact.sh` (manuell).
  - **Auf blub zu setzen (Daniel):** `export DICTATE_TEST_ROOT=/home/data/stt-test` in `~/.zshenv` (gilt auch für
    nicht interaktive Shells, also für Agenten; die Datei kommt aus chezmoi, also `chezmoi edit ~/.zshenv`); laufende Shells und Sitzungen sehen sie erst nach Neustart.

  Ursprünglicher Befund: **Prüfumgebung nur auf blub lauffähig:** `scripts/check.sh` (`LOGROOT`, Testsatz-Audio), `tests/wayland/run.sh`,
  `tests/wayland/nested/harness.sh` und einige Tests (`tests/*_real.rs`, `tests/speech_testset.rs`, `tests/ai_http.rs`)
  setzen `/home/data/stt-test` voraus. Kein Teil des Produkts (`src/diagnose/live_eval.rs` nennt den Ordner nur unter
  `cfg(test)`); betrifft aber U4: bevor `CONTRIBUTING.md` auf `check.sh` verweist, den Ordner über eine Variable
  (Vorgabe unter `XDG_CACHE_HOME`) einstellbar machen oder in CONTRIBUTING sagen, welche Schritte ohne ihn laufen.
  **Entschieden in U4 (G3):** Variable `DICTATE_TEST_ROOT` als eigener Block nach U4. Umfang (gezählt 23.11.):
  `scripts/check.sh` (`LOGROOT`, Sperre, `TESTSET_AUDIO`), `tests/wayland/run.sh`, `tests/wayland/nested/harness.sh`
  (`WL_RUN_BASE`), `tests/gui/lib.sh`, `shots.sh`, `interact.sh`, `tests/wayland/diag-shots.sh`,
  `scripts/record-testset.sh`, die Schutzprüfung `BASE` in `tests/wayland/nested/src/{clip,main}.rs` (muss gleich
  streng bleiben: Laufzeitordner unter `<root>/wl-run/`), `tests/{engine_real,stream_real,daemon_pulse_real,
  speech_testset}.rs`, `src/diagnose/live_eval.rs` (`DICTATE_DIAG_OUT`). Zu beachten: Länge der Socket-Pfade (108
  Zeichen) unter `~/.cache`; die Sperre zieht mit dem Ordner um (zwei Ordner = zwei Sperren); CONTRIBUTING, README
  und `CLAUDE.md` nachführen. Prüfung `fmt,clippy,unit,wayland,gui,speech,gpu`.
- O6 **Noctalia und rofi-emoji in der Palette** (E6): Sperre (`noctalia msg session lock`), Verlauf der Zwischenablage
  (`noctalia msg panel-toggle clipboard`) und Emoji (`rofi -modi emoji`) setzen diese Programme voraus; fehlt eines, meldet
  das Overlay „Program not found“. Vor U5 entscheiden, ob die Sperre über `loginctl lock-session` geht (vorher prüfen,
  ob Noctalia auf die Sperre von logind reagiert) und ob Zwischenablage und Emoji nur angeboten werden, wenn vorhanden.

  **O6 Prüfung vorab (23.11.2026, nur lesend, nichts gesperrt, nichts an Noctalia geändert):**
  - **Noctalia 5.5.0** (Quelltext am Tag `v5.5.0`: `src/dbus/logind/logind_service.cpp`, `src/app/application_ui.cpp`,
    `src/shell/session/session_ipc.cpp`; dazu `strings` am installierten Binary): `LogindService::ensureSessionLockMonitor`
    abonniert `Lock` und `Unlock` von `org.freedesktop.login1.Session` auf der Sitzung aus `XDG_SESSION_ID` (sonst
    `GetSessionByPID`). Der Rückruf ruft `LockScreen::lock()`, dieselbe Funktion wie `noctalia msg session lock`. Der
    Monitor läuft nur bei eingeschaltetem Sperrbildschirm (`isLockScreenEnabled`, auch nach jedem Neuladen der Config);
    dieselbe Bedingung gilt für `noctalia msg session lock` („error: lock screen disabled“). **belegt**
  - **logind** (systemd 262, `src/login/loginctl.c`, `logind-dbus.c`, `logind-session-dbus.c`): `loginctl lock-session`
    ohne Argument ruft `Lock` auf `/org/freedesktop/login1/session/auto`. `auto` ist die Sitzung des Aufrufers, sonst
    die Anzeige-Sitzung seines Users. Ein Dienst des Users (`dictate-ui.service`) sperrt so die grafische Sitzung, und
    logind sendet das Signal `Lock` an diese Sitzung. **belegt**. Auf dem Rechner des Autors gelesen: Noctalia läuft mit
    `XDG_SESSION_ID` gleich der Anzeige-Sitzung (`loginctl show-user` › `Display`); `auto` ergab für `dictate-ui`
    schon in W4 die Wayland-Sitzung (`CLAUDE.md`, Sperre in `lock.rs`). **gemessen** Polkit: die Aktion
    `org.freedesktop.login1.lock-sessions` steht auf `auth_admin_keep`, aber `bus_session_method_lock` übergibt die UID
    des Sitzungsbesitzers als `good_user` an `bus_verify_polkit_async_full`, und `bus_message_check_good_user`
    (`src/shared/bus-polkit.c`) erlaubt einen Absender mit dieser EUID ohne Polkit. Der User sperrt die eigene
    Sitzung also ohne Rückfrage. **belegt**
  - **Grenze:** Live ist nichts geprüft (die echte Sitzung wird nie gesperrt). Hört kein Sperrprogramm auf logind,
    meldet logind trotzdem Erfolg, und es geschieht nichts.

  **O6 Entscheidungen (Daniel, 23.11.2026, alle wie empfohlen; Branch `lizenz-o6`):**

  | Nr. | Frage | Entscheidung |
  |---|---|---|
  | K1 | Sperre | **logind:** immer `loginctl lock-session`. Ein Weg für jedes Sperrprogramm, das logind folgt (Noctalia, swayidle mit swaylock, hypridle); `dictate-ui` sieht das Signal `Lock` selbst. Nachteil: ohne Sperrprogramm keine Fehlermeldung. Abgelehnt: zuerst Noctalia, sonst logind (zwei Wege); unverändert |
  | K2 | Verlauf der Zwischenablage | **Erkennen und einstellbar:** neues Feld `palette.clipboard_command` (argv wie `terminal`); leer = Noctalia, wenn vorhanden; sonst ausgeblendet |
  | K3 | Emoji | **Erkennen und einstellbar:** neues Feld `palette.emoji_command`; leer = rofi mit dem Plugin rofi-emoji, wenn beides gefunden wird; sonst ausgeblendet |

  **O6 erledigt (23.11.2026, Branch `lizenz-o6` von `main` `0f37c69`; Commits `40aa3bf` Code, Tests und README,
  `a2d705c` Korrekturen aus der Durchsicht, dazu dieser Eintrag und `CLAUDE.md`):**
  - **Sperre** (`src/ui/pal_exec.rs`): `loginctl lock-session` als Kurzläufer mit festem Programmnamen; die Meldung bei
    Fehlern bleibt „Locking the screen failed: …“.
  - **Erkennung** (neu `src/palette/widgets.rs`): Ist ein Befehl eingestellt, zählt nur er: `argv[0]` im Suchpfad oder
    als absoluter Pfad, ausführbar. Sonst gilt die automatische Wahl: `noctalia` im Suchpfad bzw. `rofi` im Suchpfad
    und `emoji.so` in `ROFI_PLUGIN_PATH` (nur absolute Einträge) oder unter dem Präfix des aufgelösten rofi
    (`lib/rofi`, `lib64/rofi`, `lib/x86_64-linux-gnu/rofi`, `lib/aarch64-linux-gnu/rofi`). Gesucht wird im `PATH` der
    Sitzung, den das Panel bei jeder Anfrage mitschickt; der Daemon sucht mit denselben Regeln wie
    `ui::pal_exec::Programs::find` (neu `sources::desktop::find_program`), damit eine angebotene Anzeige nicht mit
    „Program not found“ scheitert. `ROFI_PLUGIN_PATH` kommt aus der Umgebung des Daemons (nicht im Protokoll).
  - **Ausblenden** (`src/palette/resolve.rs`, `service.rs`): `Features` hat `clipboard_history` und `emoji` neben
    `diagnose`; `Features::offers` gilt für Schlüsselwörter und für die Liste bei leerer Eingabe samt Häufigkeit. Eine
    Absicht des Modells endet mit `NoTarget::WidgetMissing` („No clipboard history found: install Noctalia or check the
    command in Settings › Palette“, für Emoji „install rofi with rofi-emoji …“). Erkannt wird in `prepare` außerhalb
    der Sperre, höchstens alle 2 s je Suchpfad und Befehle (`WIDGETS_RECHECK` wie der Prüfabstand der Quellen: jede
    Taste ist eine Anfrage, ein hängender Ordner im Suchpfad hielte sonst jede Taste auf). Der Schalter der Diagnose
    gilt weiter sofort, auch während eines Modellaufrufs.
  - **Ausführen:** ein eingestellter Befehl startet über niri `Spawn` (ein Wähler bleibt offen, bis gewählt ist);
    leer wie bisher Noctalia als Kurzläufer bzw. rofi über `Spawn`. `ExecPrefs` (Daemon und Spiegel in
    `ui::pal_exec`) tragen die beiden Felder in jeder Antwort auf `pal_resolve` und `pal_ask`.
  - **Config und Fenster:** `PaletteCfg::clipboard_command`/`emoji_command`, geprüft mit `check_argv`. Seite
    „Palette“, neue Gruppe „Clipboard and emoji“ mit zwei argv-Listen; die Prüfung beim Tippen sagt dasselbe wie der
    Daemon (Test).
  - **Tests:** `widgets.rs` (4 Tests gegen Fake-Ordner), Spiegeltest `ui_mirror` (Daemon und `Programs::find` finden
    dasselbe: Links, nicht ausführbar, Ordner, relative Pfade, doppelte Ordner), Resolver (Ausblenden an allen
    Stellen, Modell), Zwischenspeicher der Erkennung,
    Dienst (Suchpfad der Sitzung, eigener Befehl ersetzt Noctalia), `tests/daemon_palette.rs` (Ende zu Ende mit `path`
    in der Anfrage), Ausführer, Automat des Panels, Config, Fenster; `tests/wayland/palette.sh` (Sperre
    `loginctl [lock-session]`, eigene Befehle `copyq [toggle]` und `bemoji [-t]` gegen Fake-Programme).
  - **README:** Voraussetzungen (Sperrprogramm, das logind folgt; Noctalia und rofi-emoji optional), Abschnitt
    Befehlspalette (Sperre, Anzeigen nur wenn vorhanden), Tabelle `[palette]` mit beiden Feldern.
  - **Durchsicht** (Subagent, Sonnet, nur lesend; nichts Schweres): übernommen der Spiegeltest der Programmsuche, die
    Meldungen (auch für einen eingestellten, nicht gefundenen Befehl; Emoji nennt rofi), der Zwischenspeicher, im
    README der Hinweis auf Noctalias Sperrbildschirm und dass `ROFI_PLUGIN_PATH` aus der Umgebung des Dienstes kommt,
    im Daemon-Test „rofi ohne rofi-emoji“. Bewusst nicht: `SessionEnv::from_process` liest `PATH` mit `var().ok()`
    (nicht UTF-8 = fehlt, dann gilt der des Daemons; `Programs` nimmt `var_os`), sehr unwahrscheinlich und nicht O6;
    der Katalog der KI nennt `show_widget` auch, wenn beide fehlen (Schema bleibt flach und fest, die Meldung sagt,
    was fehlt); ein Tippfehler im eigenen Befehl blendet die Anzeige still aus (die Meldung kommt nur über die KI).
  - **Prüfung:** `DICTATE_TEST_ROOT=/home/data/stt-test scripts/check.sh --only fmt,clippy,unit,wayland,gui` auf
    `40aa3bf` grün (fmt 1 s, clippy 18 s, unit 488 s, wayland 511 s, gui 107 s), vorher gezielt `--lib` (palette,
    `ui::pal_exec`, `ui::cmdpal`, config) 333 ok, `--test daemon_palette` 23 ok, `--bin dictate-settings` 197 ok,
    `--bin dictate-ui` 365 ok; danach nichts übrig in `wl-run`. Nach den Korrekturen der Durchsicht (nur
    `src/palette/`, Test, README) gezielt `--lib` 335 ok, `--test daemon_palette` 23 ok, dann `check.sh --only
    fmt,clippy,unit` auf `a2d705c` grün (fmt 2 s, clippy 23 s, unit 395 s); `wayland` und `gui` nicht wiederholt (keine Änderung unter `src/ui/`, `src/gui/`, `src/bin/` oder an den Testskripten).
  - **Handtest abgenommen** (Daniel, 23.11.2026, „Handtest ok“, nach Merge `7375172`, Installation und Neustart der
    Dienste): „Lock“ aus der Palette sperrt über logind ohne Polkit-Rückfrage und Noctalia zeigt den Sperrbildschirm;
    Zwischenablage und Emoji wie bisher; die neue Gruppe „Clipboard and emoji“ auf der Seite „Palette“ angesehen.
- **U3e Nachprüfung und Neuentscheidung (23.11.2026, Sitzung nach U4; eingetragen in Block O7).**
  - **Nachprüfung, nur lesend:** 20 Commits und 73 neue Blobs seit U3 (O3, U4). gitleaks über Blobs und
    Commit-Nachrichten: 0 Funde. Eigene Muster: nur Testwerte und Doku-Adressen. Exakter Abgleich ohne Ausgabe der
    Werte (machine-id, 21 Boot-IDs, 36 MACs, 39 IPs, Tailscale, GPU-UUID, Seriennummern, Namen der
    NetworkManager-Verbindungen, Schlüsseldatei): 0 Treffer. Ordnernamen aus `/home/git`: nur eigene Projekte (wie P2),
    der Name mit Kundenbezug ist nicht dazugekommen. Rohdaten unter `/home/data/stt-test/logs/u3b/` (nicht im Repo; nach
    U5 mit `u3/` löschen, O5).
  - **Warum neu entschieden:** Muster finden nur Geheimnisse. Der Verlauf enthält auch, was kein Muster findet
    (Geschäftliches, Einrichtung von blub, einen Ordnernamen mit möglichem Kundenbezug, die E-Mail in allen Commits);
    alle Fassungen der Doku sind 405 Blobs mit 26 MB. Die GPL verlangt für ausgelieferte Binaries den Quelltext, keinen
    Verlauf.

  | Nr. | Frage | Entscheidung (Daniel, 23.11.2026, wie empfohlen) |
  |---|---|---|
  | H1 | Öffentlicher Verlauf | **Neuer Start-Commit** (ersetzt „ganzer Verlauf wird öffentlich“ aus U3): Der öffentliche Verlauf beginnt mit einem einzigen geprüften Commit des dann aktuellen Baums; der alte Verlauf bleibt privat als Archiv auf dem GitLab, die Arbeit geht in einem neuen Repository weiter, das nach GitHub gespiegelt wird. Folgen: O4 entfällt, `git-filter-repo` wird nicht gebraucht; F1 (E-Mail als Autor) gilt weiter für die neuen Commits; Hash-Verweise in den Plänen gelten nur im Archiv. Der Umzug ist der erste Schritt von U5 (etwa 1 h, außenwirksam, Freigabe je Schritt) |
  | H2 | Interne Dateien im öffentlichen Baum | **Geschäftliches raus, Technik bleibt** (ändert P6): technische Pläne, `SPEC-v2.md`, `ROADMAP.md`, `docs/` und `CLAUDE.md` bleiben; die geschäftlichen Teile dieses Plans (Abschnitt 3, Abschnitt 5 Nr. 5–8 und 12, geschäftliche Punkte unter „Später“) wandern in eine private Datei außerhalb des Repos; Hinweise zu blub in `CLAUDE.md` neutral fassen. Eigener Block vor U5 (etwa 1 h; erledigt, siehe H2 unten) |

  Reihenfolge danach: O7 → H2 → U5 (beginnt mit dem Umzug nach H1). Offen vor U5 weiter O1, O2, O6 (O6 erledigt am
  23.11., siehe U3d; O1 und O2 für U5 erledigt am 23.11., Anwaltsfrage zu O2 offen, siehe U3d).

  **H2 erledigt (23.11.2026, Branch `lizenz-h2` von `main` `25690a7`; Commits `90fb3e2` Plan und `CLAUDE.md`,
  `506c7f9` Kommentar in `src/gui/fonts.rs`, dazu dieser Eintrag):**
  - **Ort** (Daniels Wahl, wie empfohlen): eine private Datei außerhalb jedes Repos, Ordner 0700, Datei 0600; der Pfad
    steht nur im Gedächtnis der Agenten, nicht im Repo. Inhalt wörtlich nach dem Stand von `25690a7`, mit Herkunft je
    Block: Kopf des Plans mit dem Verweis auf die Recherche, Abschnitt 3 ganz, Entscheidungen Nr. 5–8 und 12 samt
    Einleitung von Abschnitt 5, die alten Fassungen der Zeilen P6 und H2, „Später“, die Stellen „aus dem Bericht“ in
    Abschnitt 1, 2 und 4 sowie die alte Fassung von „Offen 1“ in `CLAUDE.md`.
  - **Im Plan:** Titel „Lizenz und Veröffentlichung“; Abschnitt 3 und die Zeilen Nr. 5–8 und 12 als Stummel „privat,
    nicht im Repo“ (Nummern bleiben, Verweise stimmen weiter); der Stummel von Abschnitt 3 nennt, wo dessen
    lizenztechnische Folgen stehen (6(d) in Abschnitt 2 und Nr. 13, CUDA-Erlaubnis in Abschnitt 2 und U1,
    Drittlizenzen und Schriften in U2, Nr. 9); keine davon stand nur dort. „Bericht“ heißt jetzt „private Recherche“;
    whisper.cpp unter MIT ist zusätzlich durch U2 am Quelltext belegt. Die Zeilen P6 und H2 zählen die verschobenen
    Themen nicht mehr einzeln auf.
  - **Fundliste im übrigen Baum** (alle versionierten Dateien, Markdown und Code, Suche nach geschäftlichen Begriffen
    auf Deutsch und Englisch): Geschäftliches nur in diesem Plan und in `CLAUDE.md` „Offen 1“ (dort neu gefasst, ohne
    Verweis auf den Bericht). Bleibt, weil Technik oder schon öffentlich gemeint: `CONTRIBUTING.md` („paid builds“ als
    Beispiel in der Erklärung der CLA), `SPEC-v2.md` und `src/config.rs` (`support_url`, Spendenlink als Funktion), U5
    hier (Sponsors oder Liberapay als Teil des Release), Nr. 3 und 10 (Reichweite, BYOK), `diagnose-ai.md` (Preise der
    API als Messkosten), `PLAN-wayland.md` Entscheidung 23 (nennt „Lizenz und Monetarisierung“ nur als nächstes
    Thema). In `README.md`, `ROADMAP.md`, `docs/` und den übrigen Plänen nichts.
  - **`CLAUDE.md`** (Daniels Wahl: nur den Rechnernamen neutral; empfohlen war zusätzlich, Vorfall, Datum und
    Dateinamen mit dem Rechnernamen sowie die GitLab-Adresse zu streichen): Fließtext „blub“ → „Rechner des Autors“,
    dazu ein Satz am Anfang, dass Agenten auf diesem Rechner laufen. Wörtlich bleiben Dateinamen und Befehle
    (`pstore-blub.conf`, `acl-blub.conf`, `systemctl --user -M blub@.host`), die GitLab-Adresse, die Notiz zum
    Schlüssel, pstore, `sleep.conf`, espanso, chezmoi und der Testordner. „Offen 1“ neu gefasst (Stand bis H2, Regel
    „keine geschäftlichen Angaben ins Repo“, nächster Schritt U5). „blub“ in anderen Plänen, Skripten und Tests bleibt
    nach P4.
  - **Verweise:** `src/gui/fonts.rs` nannte in einem Kommentar „Abschnitt 3 Punkt 5“, jetzt „U2“; die übrigen
    Verweise auf `PLAN-lizenz.md` in Code, Skripten und Markdown zeigen auf U1, U2, O3, O7, G3, U3d, U3e oder U4, keiner
    auf Abschnitt 3 oder Nr. 5–8 und 12.
  - **Prüfung:** für die Markdown-Dateien kein Schritt von `check.sh` (kein Test liest `PLAN-lizenz.md` oder
    `CLAUDE.md`, `git grep` über `src`, `tests`, `scripts`: nur Verweise in Kommentaren; Markdown trägt keinen
    Lizenzkopf). Für den Kommentar in `src/gui/` nach der Tabelle in `CLAUDE.md`: `DICTATE_TEST_ROOT=/home/data/stt-test
    scripts/check.sh --only fmt,clippy,gui` grün (fmt 1 s, clippy 3 s, gui 153 s), `cargo test --release --bin
    dictate-settings` 197 ok, danach nichts übrig in `wl-run`. Verlust geprüft: jede aus dem Plan entfernte Zeile steht wörtlich in der privaten Datei, außer einem
    umformulierten Verweis. Abschluss-`git grep` (ohne `THIRD-PARTY-LICENSES.txt`, `licenses/`, Wortliste) nach 15
    Begriffen der verschobenen Teile (Liste in der privaten Datei): 11 ohne Treffer, die übrigen nur in anderer
    Bedeutung („MERCHANTABILITY“ in den Lizenzköpfen und `LICENSE`, ein Zufall in einer PNG, „Preis:“ als Abwägung in
    `PLAN-wayland.md` und im vkbd-Spike, API-Preise in `diagnose-ai.md`, Steuerzeichen und Steuer-Socket, Fixtures in
    `src/diagnose/pseudo.rs`) und in der allgemeinen Regel in `CLAUDE.md`.
  - **Grenze:** Der alte Verlauf enthält die geschäftlichen Teile weiter; er bleibt nach H1 privat, H2 schützt den
    künftigen Start-Commit.
- **U4 Beitragsregeln: erledigt am 23.11.2026** (Einzelheiten unten). `CONTRIBUTING` mit CLA-Hinweis, bis dahin
  keine fremden Code-Beiträge annehmen.
  **Neu entschieden (Daniel, 23.11.2026, Rückfrage nach U3 „Liegt der CLA-Text vom Anwalt vor?“, Antwort „Selber
  schreiben“):** den CLA-Text selbst entwerfen statt auf den Anwalt zu warten; das ändert Nr. 11 („Text vor dem ersten
  Beitrag mit einem Anwalt klären“). Deutung von Claude: auf Grundlage einer verbreiteten Vorlage (etwa Harmony oder
  die Individual CLA der Apache Software Foundation, Lizenz der Vorlage prüfen) mit Umlizenzierungsrecht für Daniel,
  kein Rechtsrat; ob ein Anwalt den Entwurf vor dem ersten fremden Beitrag noch liest, bei Abschluss von U4 Daniel
  fragen. Reihenfolge: erst O3 (eigener Block, Branch `lizenz-o3`), dann U4.

  **U4 Entscheidungen (Daniel, 23.11.2026, alle wie empfohlen):**

  | Nr. | Frage | Entscheidung |
  |---|---|---|
  | G1 | Vorlage der CLA | **Harmony Individual CLA 1.0, Option 5** („any license“ mit der Zusage, Beiträge auch unter der Lizenz vom Einreichungstag anzubieten); erwogen war die Individual CLA 2.2 der Apache Software Foundation (keine Zusage an Beitragende, keine Rechtswahl) |
  | G2 | Annahme der CLA | **Eintrag per Commit:** eigene Zeile in `CLA-SIGNATURES.md` in einem Commit der beitragenden Person im ersten Pull/Merge Request, feste Commit-Nachricht; Annahme durch Daniel mit dem Merge. Verworfen: cla-assistant.io (nur GitHub, fremder Dienst), `Signed-off-by` als Zustimmung (heißt üblich DCO). CLA Assistant Lite (`contributor-assistant/github-action`) ist laut GitHub-API archiviert (Stand 23.11.2026) |
  | G3 | O7 Testordner | **Variable als eigener Block nach U4:** `DICTATE_TEST_ROOT`, Vorgabe `${XDG_CACHE_HOME:-~/.cache}/dictate-test`; auf blub setzt Daniel sie selbst auf `/home/data/stt-test`. Bis dahin sagt CONTRIBUTING ehrlich, dass `check.sh` den festen Ordner braucht, und nennt die cargo-Befehle der ersten vier Schritte |
  | G4 | Rechtswahl | **Österreich** (Platzhalter `[JURISDICTION]`; UN-Kaufrecht wie in der Vorlage ausgeschlossen) |
  | G5 | KI-gestützte Beiträge | **Erlaubt, mit Offenlegung** im Pull/Merge Request (Werkzeug, betroffene Teile); die Person muss alles verstanden und geprüft haben und bleibt für die Zusicherungen der CLA verantwortlich |
  | G6 | Liest ein Anwalt den Entwurf vor dem ersten fremden Beitrag? (Rückfrage bei Abschluss von U4) | **Nein, so verwenden** (empfohlen war: Anwalt vorher). Umgesetzt (`854e73b`): Fassung `1.0`, Statuszeile in CONTRIBUTING und der README-Satz „cannot be accepted yet“ entfallen, der Hinweis „Not reviewed by a lawyer“ bleibt in `CLA.md` und CONTRIBUTING. Fremde Beiträge sind möglich, sobald es ein öffentliches Repository gibt (U5). Die Punkte „Für den Anwalt“ bleiben als Liste für einen späteren Anlass (etwa vor bezahlten Builds) |

  Nicht als Frage vorgelegt: „We“ ist Daniel persönlich (wie U1); die Entity-Fassung entfällt (Unternehmen melden sich
  vorher); beidseitige Gewährleistungs- und Haftungsausschlüsse der Vorlage aufgenommen; keine Postanschrift im Repo;
  Beitragende dürfen Kommentare, Commits und Issues englisch schreiben.

  **U4 erledigt (23.11.2026, Branch `lizenz-u4` von `main` `60984f8`; Commit `9cffaaa` Dateien, danach Plan und
  `CLAUDE.md`):**
  - **Quelle der Vorlage:** Harmony Combined Contributor Agreement Template, Version 1.0 (2011-07-04),
    <https://www.harmonyagreements.org/docs/ha-combined-v1>, abgerufen 23.11.2026 (HTML, SHA-256 `40a0136e…`),
    Lizenz laut Fuß der Seite und der Übersicht <https://www.harmonyagreements.org/agreements>: **CC BY 3.0 Unported**.
    Folgen nach Abschnitt 4(a)/(b) der Lizenz (gelesen im Rechtstext): Namensnennung mit Titel, URI und Lizenz-URI,
    Kennzeichnung als Bearbeitung, kein Anschein einer Billigung. Darum steht in `CLA.md` ein Abschnitt „About this
    text“ mit Quelle, Abrufdatum, Lizenz und allen Änderungen, und `CLA.md` steht selbst unter CC BY 3.0 (README nennt
    es als Ausnahme von der GPL). Die Apache ICLA 2.2 (<https://www.apache.org/licenses/icla.pdf>, Weiterverwendung laut
    <https://www.apache.org/licenses/cla-faq.html> erlaubt, ohne den Namen Apache) wurde gelesen, nicht genommen.
  - **Änderungen gegenüber der Vorlage** (wortweise per `difflib` gegen den abgerufenen Text geprüft: außer gestrichenen Varianten und Klammerstellen weichen nur diese ab):
    Individual- und License-Fassung; Entity, Abtretung, „Affiliates“ und „assigned or“ entfernt; Option 5, Optionen 1–4
    und die Medienliste samt Begriff „Media“ entfernt (Option 5 deckt jeden Teil); 3(c) ohne Verweis auf die
    Entity-Fassung; beidseitige Fassung in 4 und 5; Platzhalter gefüllt (dictate, Daniel Kurdoghlian, Republik
    Österreich); Anhang A (fremdes Material: vorher fragen, kennzeichnen, Hinweise behalten) und Anhang B (Zustimmung per
    Commit, Annahme per Merge, je Fassung) statt `[NONOWNER_INSTRUCTIONS]`, `[SUBMISSION_INSTRUCTIONS]` und
    Unterschriftsblöcken; Hinweis „Not reviewed by a lawyer“ und Fassungszeile (erst `1.0-draft`, nach G6 `1.0`).
  - **Dateien:** `CLA.md`, `CLA-SIGNATURES.md` (Anleitung und leere Tabelle: Name, Konto mit Host, Datum, Fassung),
    `CONTRIBUTING.md` (Status, CLA in einfachen Worten, Vorgehen, Einrichtung, Prüfschritte mit Bedarf je Schritt und
    der Einschränkung aus O7, Stil und Regeln (Datenschutz beim Loggen, keine Shell, Abhängigkeiten und Drittlizenzen),
    Lizenzköpfe aus `tests/license_headers.rs`, KI-Beiträge, fremdes Material, Sprache, Inhalt eines Requests), README
    Abschnitt „Contributing“ (ersetzt den Satz „cannot be accepted yet“).
  - **Prüfung:** reine Doku, kein Prüfschritt gelaufen (keiner berührt; Markdown trägt keinen Lizenzkopf,
    `tests/license_headers.rs` prüft nur `.rs` und Skripte). Die Befehle in CONTRIBUTING gegen `check.sh` abgeglichen;
    die nicht ignorierten Tests brauchen `/home/data/stt-test` nicht (gelesen: alle Tests mit dem Pfad tragen
    `#[ignore]`, auch `live_eval.rs`); `cargo fetch --locked` vor den Offline-Lizenzprüfungen ergänzt (nicht in einem
    frischen Klon gemessen).
  - **Für den Anwalt** (alles ungeprüft, kein Rechtsrat): (1) Nach österreichischem UrhG ist das Urheberrecht selbst
    nicht übertragbar, nur Nutzungsrechte (nach Erinnerung § 23 Abs. 3 UrhG); trägt eine einfache
    Werknutzungsbewilligung mit Unterlizenz und „jede Lizenz“ (2.1, 2.3)? (2) Verzicht auf Urheberpersönlichkeitsrechte
    (2.4) nach österreichischem Recht. (3) Vertragsschluss durch Commit und Merge (Anhang B), Minderjährige, Form der
    Zustimmung des Arbeitgebers. (4) Haftungs- und Gewährleistungsausschluss, falls Beitragende Verbraucher sind.
    (5) Lizenzkopf nennt nur Daniel, die Rechte der Beitragenden stehen im Verlauf und in der CLA. (6) Schutz
    KI-gestützt geschriebenen Codes (Nr. 11). (7) CC BY 3.0 für die bearbeitete CLA. (8) Fremdes Material unter
    Copyleft in Beiträgen (CONTRIBUTING verlangt dafür Daniels ausdrückliche Zustimmung, weil es sich nicht
    umlizenzieren lässt).
  - **Offen:** O7 als nächster Block (G3); eine Adresse für Sicherheitsmeldungen fehlt in CONTRIBUTING (mit U5, wenn es
    ein öffentliches Repository gibt).
- **U5 Release** (begonnen am 23.11.2026, Branch `lizenz-u5` von `main` `849bdcd`): GitHub-Repository, Sponsors oder
  Liberapay, Download-Seite, AUR-PKGBUILD (Quelltext, CUDA vom System). Alles außenwirksam: nur mit Daniels
  ausdrücklicher Freigabe je Schritt. Erster Schritt ist der Umzug nach H1.

  **Entscheidungen U5 (Daniel, 23.11.2026):**

  | Nr. | Frage | Entscheidung |
  |---|---|---|
  | V1 | Arbeitsort | **GitHub direkt, kein Spiegel** (Daniel: „Wenn das Projekt jetzt vollständig öffentlich ist, dann kann es auch direkt auf GitHub sein statt auf GitLab.“). Ändert Nr. 14 und H1 („neues Repository, das nach GitHub gespiegelt wird“) |
  | V2 | Besitzer und Name | **`derblub/dictate`** (persönliches Konto, wie empfohlen; später an eine Organisation übertragbar) |
  | V3 | Sichtbarkeit | **Erst privat anlegen**, Start-Commit pushen und dort prüfen, das Umschalten auf öffentlich als eigene Freigabe (wie empfohlen) |
  | V4 | Altes GitLab-Projekt | **Archivieren** (privat, nur lesbar); im alten lokalen Checkout heißt das Remote danach `archiv` statt `origin` (wie empfohlen) |
  | V5 | Sicherheitsmeldungen | **GitHub Private Vulnerability Reporting**, neue Datei `SECURITY.md`, Verweis in CONTRIBUTING und README; keine Adresse öffentlich (wie empfohlen). Schließt den offenen Punkt aus U4 |
  | V6 | Nachricht des Start-Commits | **Englisch** (empfohlen war deutsch nach der Regel „Commits deutsch“): „dictate: initial public release“ mit dem Satz zu Lizenz und Verlauf. Ausnahme nur für diesen Commit |
  | V7 | Lokaler Checkout | **Tauschen** (empfohlen war: daneben): der alte Haupt-Checkout wird `/home/git/blub/dictate-archiv`, der neue `/home/git/blub/dictate`. Erst, wenn keine Sitzung mehr in einem Worktree des alten Repos läuft; bis dahin liegt der neue Checkout im Übergangsordner `/home/git/blub/dictate-neu` |
  | V8 | Aufmachung nach der Repo-Vorgabe (Daniel: „Hier muss das genau gleich ausschauen“ wie die übrigen Repos) | **In den Start-Commit**, solange das Repo privat ist: den einen Commit neu bauen und auf GitHub per Force-Push ersetzen (eigene Freigabe), Baum danach erneut nach U5a prüfen (wie empfohlen) |
  | V9 | Leistungsseite in README-Fuß, Kontaktlink der Issue-Templates und Homepage | **`https://pushingpixels.at/creates/ki-beratung`** (wie awesome-claude-usage), Satz ohne Preis (wie empfohlen; vereinbar mit „keine geschäftlichen Angaben ins Repo“) |
  | V10 | Kontakt in SECURITY.md und Code of Conduct | **Wie die anderen Repos:** Private Reporting und die geschäftliche Adresse des Autors, Antwort binnen fünf Werktagen; dieselbe Adresse im Code of Conduct. Ändert V5 („keine Adresse“) |
  | V11 | CI | **So viel wie auf GitHub-Runnern geht:** fmt, licenses, clippy und Unit-Tests auf `ubuntu-latest` mit CUDA-Toolkit ohne GPU; Pakete klären und Laufzeit messen, bevor der Badge in die README kommt (wie empfohlen) |
  | V12 | Basis der CI (Befund U5c: Ubuntu hat weder GTK 4.24 noch gtk4-layer-shell 1.3, die `dictate-ui` mit `v4_24`/`v1_3` braucht) | **Arch-Container** auf `ubuntu-latest` (`container: archlinux:latest`, wie empfohlen): fmt, licenses, clippy und unit über alle drei Binaries. Nicht gewählt: Ubuntu mit CUDA aus NVIDIAs apt-Repo und `--no-default-features --features gui` (ohne `dictate-ui`). Ergänzt V11 |
  | V13 | CI vor dem ersten Push lokal nachstellen? | **Ja** (wie empfohlen): einmal in Docker (`archlinux:latest`, 4 Kerne), Container und Image danach weg |
  | V14 | Lizenz der Pushing-Pixels-Zeichen in `docs/assets/` | **Von der GPL ausgenommen** (wie empfohlen): Marken des Autors, alle Rechte vorbehalten; das dictate-Logo (`assets/logo/`) bleibt GPL |

  **U5a Prüfung des Baums für den Start-Commit (23.11.2026, gemessen, nur lesend).** Geprüft wird ein mit `git archive`
  exportierter Baum (nie das Arbeitsverzeichnis), zuerst der von `849bdcd` als Probe, vor dem echten Start-Commit
  noch einmal dessen Baum. Rohdaten und Skript außerhalb des Repos (unter dem Testordner, `logs/u5/`; nach U5 mit
  `u3/` und `u3b/` löschen, O5). Ergebnis für `849bdcd` (294 Dateien, 6 binär):
  - **gitleaks** über den Baum: 52 Treffer, alle in den bekannten Dateien aus S1 und S2 (Testmuster in
    `src/actions/secrets.rs`, `src/diagnose/ai.rs`, `src/diagnose/probes/text.rs`, `src/palette/ai.rs`; synthetische
    Logs in `tests/diagnose/corpus/`), keine neue Datei.
  - **Exakter Abgleich ohne Ausgabe der Werte**, Listen am selben Tag neu erhoben: machine-id, 42 Boot-IDs, 49 MACs,
    54 IP-Adressen (ohne Loopback), 24 Tailscale-Adressen, 22 Tailscale-Namen, Tailnet, GPU-UUID, 22 Kennungen der
    Platten (Seriennummer, WWN, UUID, PARTUUID), 25 UUIDs und 24 Namen der NetworkManager-Verbindungen, die
    Schlüsseldateien und lokale Tokens, die E-Mail-Adressen der Git-Konfiguration, der Name aus F2 (aus den entfernten
    Zeilen von `e2bdb6c`), Name und Ordner der privaten Datei aus H2. Treffer nur: der Rechnername (P4), ein Wort der
    EFF-Liste, das zugleich ein Gerätename ist, generische Namen (`docker0`, `tailscale0`) und generische Adressen
    (`127.0.0.1`, `172.17.0.1`, `::1`, eine verbreitete Router-Vorgabe als Testfall einer URL). Keine
    machine-id, Boot-ID, MAC, Tailscale-Adresse, GPU-, Platten- oder Verbindungskennung, kein Schlüssel, keine eigene
    E-Mail-Adresse, nicht der Name aus F2.
  - **Ordnernamen unter `/home/git`** (95, zwei Ebenen und Repositories auf der dritten): 14 Namen kommen vor, eigene
    Projekte wie in P2 und ein neuer gegenüber U3b, ein allgemeines Wort in einer Testliste von `src/diagnose/pseudo.rs`.
  - **Eigene Muster:** Schlüssel bekannter Anbieter, PEM, JWT, Zuweisungen nur in den Testdateien aus S1
    (`tests/palette_ai_http.rs` mit einem Platzhalter aus Buchstabenfolge); keine IBAN, keine Telefonnummer;
    E-Mail-Adressen nur Beispiel- und Dokudomänen, Systemd-Einheiten (`user@1000.service`) und der GitLab-Host (P5).
    100.64/10-Adressen und `*.ts.net` nur in Tests und synthetischen Logs der Diagnose.
  - **Geschäftliche Begriffe** (die 15 aus H2, dazu Anbieter- und Spendennamen): nur in anderer Bedeutung
    (Steuerzeichen, Steuer-Socket, Abwägungen, API-Preise, Stripe als Schlüsselart im Geheimnisfilter) oder in den
    Einträgen dieses Plans, die H2 schon nennt.
  - **Binärdateien:** Logo und 5 Screenshots aus dem verschachtelten Test-niri (S4); Metadaten nur Zeitstempel.
  - **Versteckte Dateien:** nur `.gitignore`. Kein eigenes `build.rs`, kein Skript liest Git: ein Build hängt nur am Baum.

  **U5b Start-Commit (Probe, 23.11.2026):** in einem leeren Ordner außerhalb des Repos `git init`, Baum per
  `git archive 849bdcd` ausgepackt, ein Commit ohne Remote. Baum-Hash gleich dem von `849bdcd` (`d95a6e09…`), Autor
  und Committer (Name und E-Mail) gleich denen von `849bdcd` (F1; in einem frischen Ordner gilt sonst die globale
  Git-Konfiguration mit anderer Adresse, darum dort gesetzt), genau ein Commit. Gleicher Baum heißt gleicher
  Quelltext wie der warme Build im Worktree; ein Release-Build im frischen Ordner bräuchte den CUDA-Erstbau (etwa
  10 min) ohne neuen Befund. Die Probe wird nicht veröffentlicht.

  **Ablauf danach** (jeder außenwirksame Schritt einzeln mit Daniels Freigabe):
  1. **Erledigt am 23.11.2026:** Branch `lizenz-u5` (`c827fc4`) im alten Repo nach `main` gemergt (`68ca88f`) und zum
     alten GitLab gepusht (Archiv vollständig bis zum Start). Baum gleich dem in U5a geprüften. Daraus der
     Start-Commit `ef6cd55` im Übergangsordner: Baum-Hash gleich `68ca88f` (`f269143…`), Autor und Committer nach F1,
     Nachricht nach V6, ein Commit, keine Tags. `scripts/check.sh --only fmt,licenses` in der Probe mit gleichem Baum
     grün (fmt 3 s, licenses 2 s). Nach V8 durch einen neu gebauten Start-Commit mit dem Baum aus U5c ersetzt (gleiche
     Nachricht und Identität; der Hash steht nicht im Baum, den er beschreibt).
  2. **Erledigt am 23.11.2026:** `derblub/dictate` privat angelegt (`gh repo create`, ohne Wiki, ohne Dateien von
     GitHub), `ef6cd55` nach `main` gepusht. Geprüft über die API: privat, nur `main`, keine Tags, 1 Commit, Baum
     `f269143…` wie lokal, 295 Dateien, Lizenz als GPL-3.0 erkannt, README und CONTRIBUTING erkannt. `ef6cd55` wird nach
     V8 per Force-Push ersetzt (eigene Freigabe).
  2a. **U5c Aufmachung nach der Repo-Vorgabe (umgesetzt am 23.11.2026, V8–V14; Ergebnis unter der Liste).** Vorgabe: die öffentlichen eigenen Repos, Referenz
     `/home/git/blub/django-upgrade-report` (lokal und `derblub/django-upgrade-report`), dazu der Design Guide von
     Pushing Pixels (Artefakt, `project/README.md`, Abschnitt „Voice“) und der Block B16 in `PLAN.md` von niri-coach
     (T41–T43). Gemessen am 23.11.2026 (API und lokale Checkouts von django-upgrade-report, changereel, axe-routes,
     postkasten, ha-tonie-sync, awesome-claude-usage): alle haben das folgende Muster, dictate bisher nur `LICENSE`,
     `README.md`, `CONTRIBUTING.md`, `SECURITY.md`.
     - **README-Kopf:** `<div align="center">`, Zeichen über `<picture>` mit `docs/assets/logo-dark.svg`,
       `logo-light.svg`, `logo.svg` (Breite 96, Link auf pushingpixels.at), `<h1>dictate</h1>`, Tagline fett, Badges
       (CI, Lizenz GPL-3.0-or-later, weitere nur mit Beleg), Sprunglinks mit „·“, dann `---`. Screenshots oder GIF
       (Overlay, Einstellungsfenster, Palette, Diagnose; aus `tests/gui/shots.sh` bzw. dem verschachtelten niri, nur
       Testdaten).
     - **README-Ende:** „Contributing“ mit Verweis auf CONTRIBUTING, Code of Conduct und SECURITY; „License“ wie
       Referenz (`[GPL-3.0-or-later](LICENSE) © Daniel Kurdoghlian, Pushing Pixels`, CUDA-Erlaubnis und
       Drittlizenzen bleiben erwähnt); Fuß mit `docs/assets/pushing-pixels-dark.svg`/`-light.svg`/`.svg` (Breite 320),
       „Built and maintained by … at Pushing Pixels in Vienna.“ und eine Angebotszeile nach V9.
     - **`docs/assets/`:** die sechs SVG-Dateien byte-gleich aus dem Referenz-Repo kopieren (nie neu zeichnen),
       `social-preview.png` 1280×640 nach dem Guide (Quelle wie `handoff/brand/social.html` im Ordner
       `django-upgrade-report-handoff`; Daniel lädt sie in den Einstellungen hoch).
     - **Begleitdateien:** `CHANGELOG.md` (Keep a Changelog, Abschnitt für den ersten öffentlichen Stand),
       `CODE_OF_CONDUCT.md` (Contributor Covenant wie Referenz, Kontakt nach V10), `SECURITY.md` nach V10 im Aufbau der
       Referenz (Supported versions, Reporting, was dictate mit Daten tut: Verweis auf README „Privacy and security“),
       `.github/ISSUE_TEMPLATE/bug.yml`, `feature.yml`, `config.yml` (`blank_issues_enabled: false`, Kontaktlinks
       Security advisory und Leistungsseite nach V9), `.github/pull_request_template.md` (Checkliste mit
       `scripts/check.sh`, CHANGELOG, CLA-Zeile), `.github/dependabot.yml` (cargo und github-actions, monatlich,
       gruppiert), `.github/workflows/ci.yml` nach V11.
     - **Sprache der öffentlichen englischen Dateien** (README, CONTRIBUTING, SECURITY, CHANGELOG, Templates): Ich-Form,
       keine Gedankenstriche (README hat 15), keine Emoji, keine Werbefloskeln; `CLA.md` bleibt (Rechtstext nach Harmony,
       „We“ ist dort die Vertragspartei). Prüfung: `grep -rnP '[\x{2013}\x{2014}]' -- '*.md'` ohne die internen deutschen Dateien.
     - **Einstellungen** (außenwirksam, je eine Freigabe): Homepage nach V9, Topics (8–12, Vorschlag vorlegen),
       Dependabot-Sicherheitsupdates, Secret Scanning mit Push Protection, Private Reporting (nach dem Umschalten),
       Social Preview (Daniel), später ein Release.
     - **Danach:** Start-Commit neu bauen (Baum = gemergter Stand, Autor nach F1, Nachricht nach V6), U5a über den
       neuen Baum, Force-Push nach Freigabe, CI auf GitHub abwarten und Laufzeit festhalten.

     **U5c umgesetzt (23.11.2026, Branch `u5-stand` im neuen Checkout, nur lokal; Zwischen-Commits bleiben lokal, der
     Baum geht nach V8 in den neu gebauten Start-Commit):**
     - **Aufräumen vorab:** vorige Sitzung ohne Ort (geschlossen), keine dictate-Worker oder -Runs in Orca, Worktrees nur
       der alte Haupt-Checkout und `phase6-abschluss` (bleibt wegen `target/`); den Chat-Tab der vorigen Sitzung kann
       die CLI nicht schließen. Keine Testumgebung lief, `wl-run` leer, Sperre frei.
     - **README:** Kopf wie die Referenz (Zeichen über `<picture>`, `<h1>`, fette Tagline, Lizenz-Badge, Sprunglinks
       mit „·“, `---`), danach die Einleitung und das Overlay-Bild; je ein Bild bei Einstellungsfenster (dunkel und
       hell über `<picture>`), Textaktionen, Befehlspalette und Diagnose, alle mit Alt-Text. Fuß wie die Referenz
       (Contributing mit Links auf CONTRIBUTING, Code of Conduct, SECURITY; „License“ mit
       `[GPL-3.0-or-later](LICENSE) © Daniel Kurdoghlian, Pushing Pixels`, CUDA-Erlaubnis, EFF-Liste, CLA unter CC BY
       3.0 und Drittlizenzen bleiben, neu die Ausnahme nach V14; Schriftzug mit „Built and maintained by …“ und die
       Angebotszeile nach V9 wortgleich wie awesome-claude-usage). Abschnitt „Development“ nennt die CI. Der
       CI-Badge kam erst nach dem grünen Lauf auf GitHub dazu (V11, zweiter Neubau).
     - **`docs/assets/`:** die sechs SVG byte-gleich aus django-upgrade-report (`cmp`); `social-preview.png` 1280×640
       im Stil der neueren Repos (awesome-claude-usage: dunkel, Titel in Großbuchstaben, Zeichen oben links, Karte
       rechts, Fuß mit Schriftzug), aus einer HTML-Quelle mit Chromium headless gerendert (Quelle und Schriften außerhalb
       des Repos unter `$DICTATE_TEST_ROOT/logs/u5/social/`, wie bei der Referenz im Handoff-Ordner). Hochladen in den
       Einstellungen macht Daniel.
     - **Bilder:** neu `tests/wayland/readme-shots.sh` (manuell, kein Prüfschritt) mit `tests/wayland/demo_daemon.py`:
       lädt den Mock des Schritts `wayland` unverändert und ersetzt nur Inhalte (englische Beispielsätze, erfundene
       Namen und Hosts, `host-1` und `user-1` als Platzhalter der Diagnose); Ausschnitt um Panel (Lage aus der niri-IPC)
       bzw. Overlay (Unterschied zu einem Bild ohne Overlay). Einstellungsfenster über `tests/gui/lib.sh` mit den
       Fixtures (dunkel und hell), Zeiger in der Ecke, Ecken transparent. Alle Bilder selbst angesehen: keine Testmarker,
       kein Rechnername, keine echten Pfade oder Kennungen, keine Textchunks in den PNG.
     - **Begleitdateien:** `CHANGELOG.md` (Keep a Changelog, „Unreleased“ als erster öffentlicher Stand),
       `CODE_OF_CONDUCT.md` byte-gleich mit der Referenz (gleich in postkasten, ha-tonie-sync, awesome-claude-usage;
       Kontakt die geschäftliche Adresse nach V10), `SECURITY.md` im Aufbau der Referenz (Supported versions, Reporting
       mit Private Reporting und Adresse, fünf Werktage, Scope wie bisher, „What dictate does with your data“ mit Verweis
       auf die README), `.github/ISSUE_TEMPLATE/` (`bug.yml`, `feature.yml`, `config.yml` mit Security advisory und der
       Leistungsseite nach V9), `.github/pull_request_template.md` (Checkliste mit `check.sh`, CHANGELOG, KI, CLA),
       `.github/dependabot.yml` (github-actions und cargo monatlich, gruppiert; `niri-ipc` und die drei Wayland-Crates
       ausgenommen, weil absichtlich gepinnt). Ein Cargo-Update von Dependabot scheitert am Schritt `licenses`, bis
       `THIRD-PARTY-LICENSES.txt` neu erzeugt ist (gewollt).
     - **CI nach V11/V12:** `.github/workflows/ci.yml` wie die übrigen Repos zwei Jobs `lint` (`check.sh --only fmt`,
       `licenses`, `clippy`, je ein Schritt) und `test` (`check.sh --only unit`), beide `container: archlinux:latest` mit
       eigenem Cache für Registry und `target/`, `CMAKE_CUDA_ARCHITECTURES=86` (Daniel 24.11.: „CI von der Seite passt
       noch nicht“; vorher ein Job `check` mit einem Sammelschritt), Pakete in
       `scripts/ci-setup-arch.sh` (base-devel, git, rustup, cmake, clang, cuda, gtk4, gtk4-layer-shell, libpulse,
       python, cargo-deny; `libcuda.so.1`, das alles mit der Engine als NEEDED hat, gehört zum Treiber und kommt nicht
       mit `cuda`: eingebunden wird der Stub des Toolkits über `ld.so.conf.d`). Befund vorher: `readelf -d` auf die warmen Binaries (dictate: libcuda, libcudart, libcublas,
       libpulse; dictate-ui: GTK 4, gtk4-layer-shell). **Lokal nachgestellt (V13)** mit
       `$DICTATE_TEST_ROOT/logs/u5/ci-local.sh` (Docker `archlinux:latest`, 4 Kerne, Baum ohne `target/` in den Container
       kopiert; Container und Image danach entfernt): Lauf 1 scheiterte in `check.sh` vor dem ersten Schritt, weil
       `our_modules` ohne Pulse-Server unter `pipefail` abbrach (behoben: ohne Server gibt es keine eigenen Module).
       Lauf 2: Setup 400 s (Pakete, Toolchain, `cargo fetch`; `/opt/cuda` 5,4 GiB, `target/` danach 2,2 GiB, Cargo 0,8,
       rustup 0,6), fmt 1 s, licenses 2 s, clippy 753 s, unit nach 694 s rot in
       `daemon_palette::the_start_moves_an_old_frecency_file_and_keeps_a_color_file`. Ursache ein Wettlauf im Test, nicht
       im Produkt: der Daemon bindet den Socket vor `frecency::adopt_legacy` und nimmt erst danach an, der Test prüfte
       die Datei, sobald der Socket da war (strace: `statx` des Tests vor `ftruncate` des Daemons; im Container jedes
       Mal, lokal nie gesehen; nicht an overlayfs und nicht an root gebunden, beides geprüft). Korrektur in
       `start_seeded`: erst eine Antwort auf `status` abwarten. Danach alle 24 Test-Binaries des neuen Checkouts im
       Container ausgeführt (Host-Bibliotheken nur lesend eingehängt, als root wie die CI): alle grün (ein Lauf als uid
       1000 scheiterte nur an `system_lookups_do_not_fail`, weil diese uid im Container keinen passwd-Eintrag hat; die CI
       läuft als root). Lauf 3 (Quelle `git archive` des Start-Commits, eingehängt wie auf GitHub unter
       `/__w/dictate/dictate`, `HOME=/github/home`, `CI=true`) fand einen zweiten, vom ersten verdeckten Fehler:
       `daemon_pulse_real` startete nicht, `libcuda.so.1` fehlte (`cuda` zieht `opencl-nvidia`, das nur zlib braucht;
       `libcuda.so.1` steht in `nvidia-utils`, 1 GB). Eingebunden ist jetzt der Stub des Toolkits
       (`/opt/cuda/lib64/stubs/libcuda.so`, SONAME `libcuda.so.1`) über `ld.so.conf.d`; vorher alle 24 Test-Binaries im
       Container mit dem Stub statt des Treibers grün. Lauf 4 aus dem Commit mit dieser Änderung: **grün**, Setup 355 s,
       fmt 3 s, licenses 1 s, clippy 871 s, unit 641 s, gesamt 1873 s (31 min, ohne Cache, 4 Kerne). Danach änderte sich
       nur dieser Plan.
     - **Force-Push (a), Freigabe Daniel 24.11.2026:** `git push --force-with-lease=main:ef6cd55 origin <start>:main`
       mit dem neu gebauten Start-Commit (Baum `9886b42…`, 319 Dateien). Über die API geprüft: `main` = der neue Commit,
       keine Eltern, Baum gleich lokal, nur `main`, keine Tags, privat. **CI auf GitHub grün** (Lauf ohne Cache): gesamt
       3516 s (59 min; Container 9 s, Pakete und Toolchain 104 s, fmt 2 s, licenses 1 s, clippy 1857 s, unit 1518 s),
       Cache danach 0,95 GB. Der Runner ist etwa doppelt so langsam wie der lokale Container (31 min). Dependabot legte
       gleich PR #1 an („bump the cargo group with 9 updates“, Branch `dependabot/cargo/…`); dessen CI scheitert wie
       angekündigt nur an `licenses` („out of date: THIRD-PARTY-LICENSES.txt“). Der PR bleibt offen für Daniel (Merge
       erst mit neu erzeugter Datei); github-actions hatte nichts zu heben.
     - **Einstellungen (b), Freigabe Daniel 24.11.2026:** Homepage nach V9, Beschreibung (Tagline der README), 12 Topics
       (dictation, speech-to-text, whisper, whisper-cpp, wayland, niri, linux, cuda, rust, gtk4, voice-typing,
       command-palette), Dependabot-Alerts und -Sicherheitsupdates an. Secret Scanning mit Push Protection ist bei einem
       privaten Repo des persönlichen Kontos nicht verfügbar (`security_and_analysis` = `null`), darum nach dem
       Umschalten (c), zusammen mit Private Reporting; erwartet sind dann Alerts zu den bekannten Testmustern.
     - **Zweiter Neubau (Daniels Wahl 24.11.2026):** CI-Badge in der README und diese Zahlen kommen in einen nochmals neu
       gebauten Start-Commit (Force-Push mit eigener Freigabe), damit der öffentliche Verlauf ein Commit bleibt.
     - **Sprache:** README, CONTRIBUTING, SECURITY, CHANGELOG und Templates ohne Gedankenstriche (`grep -nP
       '[\x{2013}\x{2014}]'` leer; Zahlenbereiche mit „to“), ohne Emoji; CONTRIBUTING in Ich-Form statt „the
       maintainer“ und mit Verweis auf die CI. `CLA.md` und `CLA-SIGNATURES.md` unverändert.
     - **Prüfung:** CUDA-Erstbau im neuen Checkout `cargo build --release --locked` grün in 1143 s (19 min, Last
       anderer Sitzungen um 6 bis 9), Injektor 9 s; `DICTATE_TEST_ROOT=/home/data/stt-test scripts/check.sh --only
       fmt,licenses,unit` grün (fmt 2 s, licenses 1 s, unit 516 s; `license_headers` findet die zwei neuen Skripte).
       Nach der Korrektur des Tests `fmt,clippy,unit` grün (fmt 2 s, clippy 51 s, unit 29 s). Release-Build im Worktree
       `phase6-abschluss` (Baum `f269143…` gleich `ef6cd55`, warmer Cache) `cargo build --release --locked` grün in
       100 s (Log `$DICTATE_TEST_ROOT/logs/u5/release-build.log`).
     - **U5a über den fertigen Baum** (`git archive` von `u5-stand`, 319 Dateien, 13 binär; Rohdaten
       `$DICTATE_TEST_ROOT/logs/u5/scan-u5c.json`, `gitleaks-u5c.json`, Werte nie ausgegeben), verglichen mit dem Stand
       von `ef6cd55`: gitleaks 52 Treffer wie vorher, keine neue Datei. Exakter Abgleich neu nur erwartete Treffer: die
       geschäftliche Adresse in `SECURITY.md` und `CODE_OF_CONDUCT.md` (V10); vier Stellen mit dem GitHub-Namen
       `derblub`, der den Rechnernamen enthält (P4; Links auf Issues und Advisories); eine Stelle mit dem schon in U5a
       genannten Gerätenamen, der ein allgemeines Wort ist (in der Adresse des Lizenz-Badges); eine verschobene Zeile in
       `tests/daemon_palette.rs` (bekannter Testwert). Keine neue machine-id, Boot-ID, MAC, IP, Tailscale-Adresse, GPU-,
       Platten- oder Verbindungskennung. Ordnernamen unter `/home/git`: sieben neue, alle eigene öffentliche Repos der
       Vorgabe (im Plan genannt). Muster: neu `pushingpixels` in Lizenzköpfen, Marken-SVG und Links, `/home/sam` und
       `/home/user-1` als erfundene Pfade in `demo_daemon.py`. Geschäftliche Begriffe: nur ein weiteres „Preis“ in
       diesem Plan (Abwägung). Neue Binärdateien: die sieben PNG in `docs/assets/` (selbst angesehen, ohne Textchunks).
       Gedankenstriche in den öffentlichen englischen Dateien: 0; alle Bildverweise der README vorhanden.
  3. Auf öffentlich schalten, Private Vulnerability Reporting einschalten (**Annahme:** geht erst bei öffentlichen Repositories; nicht getestet, weil ein Versuch ein außenwirksamer Aufruf wäre).
     **Wartet auf Daniel** (Wahl „Erst selbst ansehen“, 23.11.2026).
  4. Altes GitLab-Projekt archivieren, Remote im alten Checkout umbenennen; Checkouts tauschen (V7), Orca und Worktrees
     nachziehen, der CUDA-Erstbau fällt im neuen Checkout einmal an.
  5. Danach eigene Fragen: Sponsors oder Liberapay, Download-Seite, AUR, Ankündigung.
- **Später (Auslöser „bezahlte Builds“):** Schätzung und Plan für Hyprland, sway und Vulkan (Nr. 3); die
  geschäftlichen und rechtlichen Punkte des Verkaufs stehen privat, nicht im Repo.
