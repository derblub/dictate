# UX-Konzept: Aktions-Panel (Mod+i)

> **Archiv (W7, 21.11.2026):** Beschreibt den Stand unter AwesomeWM/X11; AwesomeWM, die X11-Wege und die CLIs sind
> entfernt, dictate läuft unter niri (Wayland). Gültig sind `README.md` und `PLAN-wayland.md`; der Inhalt hier bleibt
> unverändert.

## Stand der Umsetzung (23.10.2026)

Umgesetzt nach Abschnitt 2 bis 5 und den Entscheidungen unten: `awesome/dictate/actions.lua` (Panel),
`awesome/dictate/markdown.lua` (Markdown-Teilmenge, reines Lua, `awesome/test/markdown_test.lua`),
`awesome/dictate/widget.lua` (Leistensymbol), `init.lua` (`dictate.actions()`, `dictate.bar_widget(s)`, Tippen in
Stücken mit Fortschritt, `stop_typing`), Rust: Wortdiff `src/actions/diff.rs` (Daemon, `diff`-Zeilen vor `result`),
`act copy --block N` (`src/actions/markdown.rs`), lesbare Modusnamen. Dotfiles (chezmoi): Segment im Theme vor der
Bell, Eintrag „Text actions“ im Desktop-Menü.

Prüfung der drei aus dem Code abgeleiteten Annahmen in Xephyr (awesome v4.3-1739-gc104846a2):
1. `client.connect_signal("button::press")` feuert bei einem Klick in ein Fenster, sofern Taste 1 gebunden ist (echte
   `clientbuttons` tun das). Bestätigt; zusätzlich hängt das Modul vor `awful.util.dismiss_all_popups` (Desktop-Klick,
   und `clientbuttons` ruft sie ebenfalls).
2. Ein Klick ins Panel (`awful.popup`) ändert weder `client.focus` noch den X-Eingabefokus; danach getippter Text landet
   im Zielfenster. Bestätigt.
3. `Mod+Ctrl+e` leerte die Tipp-Warteschlange **nicht** (Tippen lief nach dem Druck weiter, 9 → 24 Zeichen). Behoben:
   `do_cancel` ruft `stop_typing` (Warteschlange leeren, laufendes xdotool per PID beenden); Replace tippt in Stücken
   von etwa 40 Zeichen und meldet Fortschritt und Abbruch im Overlay.

Abweichungen und Ergänzungen:
- Markdown wird im Panel (Lua) dargestellt, nicht im Daemon: nur so sieht schon der Stream lesbar aus. Die
  Codeblock-Regeln sind in Lua und Rust gleich (gleiche Testfälle), damit `act copy --block N` denselben Block trifft.
  Der Wortdiff liegt im Daemon (Rust, Invariante und Minimalität getestet), weil nur dort Markierung und Ergebnis
  zusammen vorliegen und Lua dafür keine Tests hätte.
- Lesbare Modusnamen ohne neues Config-Feld: feste Namen für die eingebauten Modi, eigene Modi aus dem Namen
  („my_mode“ → „My mode“). Ein `label` in `[ai.modes]` (Soll 2) bleibt offen.
- Änderungen markiert: Rephrase und KI-Modi außer `to_en`/`to_de`/`prompt`; Translate und Refine prompt zeigen den
  sauberen Text. Ist mehr als die Hälfte geändert, schickt der Daemon keinen Diff (markierter Text wäre schwerer zu
  lesen als der neue).
- Esc während der Arbeit stoppt und kehrt zur Auswahl zurück (Tabelle 2), schließt also nicht mehr sofort.
- Pango-Schriften: Achsen standen vor der Größe („Familie @wght=350 12“) und wirkten darum nie (auch im Overlay);
  jetzt „Familie 12 @wght=350“.
- Nicht umgesetzt: Zeilennummern der Geheimnisse (Kann 1), Panel am Mauszeiger (Kann 3), schwebendes Symbol (Kann 4),
  zuletzt benutzte Aktion vorausgewählt (Kann 5), Glühen des Primärknopfs (in Awesome nur mit eigener Zeichnung).
- Offen (Entscheidung des Users): Replace bei Zeilenumbrüchen (Shift+Return oder Zwischenablage); bis dahin Hinweis
  im Ergebnis, dass Copy in Chat-Feldern sicherer ist.

Anlass (User, Live-Test 23.10.2026): „Die Textaktionen sollten auch mit der Maus bedienbar sein. Auch das Schließen und
Öffnen. Grundsätzlich sollte an der User Experience noch gearbeitet werden.“ Vorgabe: keine Abkürzungen, fachlich beste
Lösung. Grundlage: `awesome/dictate/actions.lua` (Stand 21c9fc5/b6ff9f6), Screenshots
`/home/data/stt-test/logs/v2d-t5-actions-*.png`, `DESIGN.md`, `PLAN-actions.md`, `~/.config/awesome/rc.lua` und
`themes/qubit/theme.lua` (nur gelesen). Die Sicherheitsregeln aus `PLAN-actions.md` bleiben unverändert; wo ein
Vorschlag sie berührt, steht es dabei.

Mockups (gerendert aus HTML im Pushing-Pixels-Design, 2-fach):

| Zustand | Datei |
|---|---|
| Auswahl, Hover auf einer Aktion | `/home/data/stt-test/logs/ux-actions-select.png` |
| Arbeiten, Streaming | `/home/data/stt-test/logs/ux-actions-working.png` |
| Ergebnis Antwort (Markdown, Scrollleiste) | `/home/data/stt-test/logs/ux-actions-result-answer.png` |
| Ergebnis Ersetzen (Änderungen markiert) | `/home/data/stt-test/logs/ux-actions-result-replace.png` |
| Sperre wegen Geheimnis | `/home/data/stt-test/logs/ux-actions-secret.png` |
| Fehler mit „Try again“ | `/home/data/stt-test/logs/ux-actions-error.png` |
| Leerzustand „Nothing selected“ | `/home/data/stt-test/logs/ux-actions-empty.png` |

## 1. Bestandsaufnahme

Das Panel ist ein `awful.popup` (deckend, `black-dark`, 640 px, mittig auf dem Screen des fokussierten Clients) mit
einem `awful.keygrabber`. Es kennt keine einzige Maus-Bindung, keinen Hover und keine Schaltfläche. Zustände: grabbing
(nach 150 ms sichtbar), select, working (mit/ohne Streaming), result, error.

### Schwächen

**Ablauf**
- A1 Nur Tastatur. Aktionen, Copy, Replace, Schließen und Scrollen gehen nicht mit der Maus; ein Klick auf das Panel tut
  nichts, ein Klick daneben auch nicht (Panel bleibt offen und hält den Tastatur-Grab, bis Esc oder 300 s).
- A2 Kein Weg zurück: nach einem Ergebnis kann man keine zweite Aktion auf dieselbe Markierung anwenden, man muss
  schließen (das löscht die Sitzung) und neu markieren bzw. Mod+i drücken.
- A3 Fehler sind Sackgassen: nur „Esc close“, kein „Try again“, obwohl die Sitzung im Daemon noch lebt (TTL 300 s).
- A4 Die Primäraktion passt nicht zur Aktion: Enter kopiert immer, auch bei Ersetzen-Aktionen (Rephrase, Translate,
  Modi). Ersetzen liegt auf `r`, also auf derselben Taste, die im Auswahlzustand Rephrase startet. Zweimal `r` (oder
  ein gehaltenes `r`) = Rephrase und, sobald das Ergebnis da ist, sofort Ersetzen, ohne dass der Text gelesen wurde.
  Gleiches für `c` (Explain code, dann Copy).
- A5 Ob eine Aktion antwortet oder ersetzt, sieht man vorher nicht. Die Liste mischt beides.

**Rückmeldung**
- R1 Copy schließt das Panel kommentarlos; ob etwas in der Zwischenablage liegt, sieht man nicht.
- R2 Replace schließt und tippt; bei langen Texten (12 ms je Zeichen: 3000 Zeichen = 36 s) gibt es keinen Fortschritt
  und keinen sichtbaren Abbruch. Ist Replace gesperrt, steht nur ein grauer Satz in der Fußzeile.
- R3 Nicht gestreamte Modi zeigen „Working… Esc cancels“ ohne Zeitangabe, bis zu 120 s lang.
- R4 Beim Streaming wandert das Panel: `render()` zentriert bei jeder Höhenänderung neu (`awful.placement.centered`), das
  Panel springt also mit jeder neuen Zeile um eine halbe Zeile nach oben. Auch zwischen den Zuständen (select hoch,
  working flach, result hoch) springt es.

**Lesbarkeit und Hierarchie**
- L1 Das Modell steht in jeder Zeile (fünfmal `claude-haiku-5-5`), Rauschen statt Information. Wichtig ist es nur, wo es
  abweicht.
- L2 Modus-Bezeichnungen sind interne Namen („Mode: clean“, „Mode: to_de“, aus `src/actions/mod.rs`
  `format!("Mode: {name}")`).
- L3 Antworten kommen als Markdown (Listen, `code`, Codeblöcke, **fett**) und werden roh gezeigt, Sternchen und
  Backticks inklusive.
- L4 Die Vorschau schneidet nach 6 Zeilen ohne Hinweis ab; man weiß nicht, ob man alles sieht.
- L5 Scrollhinweis als Text „↑↓ lines 16 to 31 of 60“ in der Fußzeile; keine räumliche Anzeige, wo man im Text ist.
- L6 Die Fußzeile trägt zu viel: Hinweis, Tasten und Zustand in einer grauen Zeile mit Mittelpunkten.

**Fehlertexte, Leerzustand, Sperre**
- F1 „Nothing selected“ in Rot mit Fehler-Slot. Das ist kein Fehler, sondern ein Leerzustand; es fehlt, was zu tun ist.
- F2 „The dictate daemon is not running“ ohne Hinweis, wie man ihn startet.
- F3 Geheimnis-Sperre: ein roter Satz, darunter alle Aktionen auf 40 % gedimmt (unter 4,5:1, also bewusst unlesbar, aber
  trotzdem Platz fressend). Was man tun kann (Geheimnis aus der Markierung nehmen, lokalen Server nutzen), steht nicht da.
  Der Kopf sagt „Nothing is sent to api.anthropic.com“ und der rote Satz noch einmal dasselbe.
- F4 „The action was cancelled“ erscheint als roter Fehler, obwohl der User selbst abgebrochen haben kann.

**Cloud-Vorschau**
- C1 Gut: Host fett, Zeichenzahl, Vorschau, „Pressing a key sends the selection“. Mit Maus fehlt die Entsprechung
  („Click sends“), und vor dem Klick sieht man nicht, welche Aktion welches Modell nutzt, außer man liest die Spalte.

**Position, Größe, Mehrmonitor, Fokus**
- P1 Mittig auf dem Screen, nicht beim Zielfenster. Auf breiten Monitoren liegt das Panel weit weg vom Text, um den es
  geht.
- P2 Mehrmonitor: der Screen folgt `client.focus`, nicht der Maus. Öffnet man künftig per Maus auf dem anderen Monitor,
  erscheint das Panel dort, wo man nicht hinschaut (siehe 3.).
- P3 Während das Panel offen ist, gehört die Tastatur dem Panel. Klickt man ein anderes Fenster an, bekommt es den Fokus,
  aber keine Tasten (Keygrabber läuft weiter). Das wirkt wie ein Hänger.
- P4 Fokuswechsel bei offenem Ergebnis macht Replace still unmöglich (richtig), aber das Panel bleibt stehen.

## 2. Interaktionsmodell Maus und Tastatur

### Grundsätze

1. **Ein Hervorhebungsmodell für beide Eingaben.** Eine Zeile oder ein Knopf ist „aktiv“, ob durch Hover oder durch
   ↑/↓/Tab. Aktiv heißt: Fläche `black-extra`, 7-px-Teal-Pixel vor der Zeile (Marke „Tag-Knoten“ wie in der Seitenleiste),
   Rahmen des Tasten-Chips Teal 40 %. Enter löst das Aktive aus, Klick das Angeklickte.
2. **Auslösen beim Loslassen**, nur wenn Druck und Loslassen auf demselben Element liegen (Standard, verhindert
   Fehlklicks beim Ziehen). Knöpfe zeigen beim Druck kurz den Aktiv-Zustand.
3. **Totzeit nach jedem Zustandswechsel: 300 ms**, in der weder Tasten noch Klicks auslösen. Danach zählt nur ein neuer
   Tastendruck (der Keygrabber merkt sich gedrückte Tasten; ein Autorepeat oder eine Taste, die vor dem Wechsel gedrückt
   wurde, löst nichts aus). Das schließt A4 (doppeltes `r`) und Doppelklicks, die durch das neue Layout auf einem anderen
   Knopf landen.
4. **Die Primäraktion folgt der Aktion:** Ersetzen-Aktionen haben „Replace“ als Teal-Knopf auf Enter, Antworten „Copy
   answer“. Nebenaktionen sind Sekundärknöpfe mit sichtbarer Taste.
5. **Light dismiss:** Klick außerhalb schließt, wie Esc, in jedem Zustand (laufende Aktion wird abgebrochen, Sitzung
   gelöscht). Das ist die erwartete Bedeutung, und es hält die Sicherheitsregel „jedes Schließen ruft `act clear`“.
6. **Keine Textauswahl im Panel** (siehe unten), stattdessen Copy-Knöpfe für das Ganze und je Codeblock.

### Tabelle

| Zustand | Maus | Tastatur | Ergebnis |
|---|---|---|---|
| alle | Klick auf ✕ (oben rechts, 32 × 32) | Esc | schließen, laufendes `run` abbrechen, `act clear` |
| alle | Klick außerhalb des Panels (Client, Desktop, anderes Panel) | – | wie Esc (siehe „Klick außerhalb“) |
| alle | Klick auf das dictate-Symbol in der Leiste | Mod+i | schließen (Umschalter) |
| select | Hover auf Aktion: aktiv; Fußzeile nennt, was gesendet wird | ↑/↓, Tab/Shift+Tab, Home/End | Hervorhebung |
| select | Klick auf Aktion | Buchstabe der Aktion; Enter auf aktiver Zeile | `act run` (bei Cloud = Bestätigung, wie heute) |
| select | gesperrte Aktion: kein Hover, Cursor normal, Tooltip-Zeile mit Grund | Taste ignoriert, Fußzeile nennt den Grund | nichts |
| select | Klick „Settings“ | – | Panel schließen, `dictate-settings --page actions` |
| select | Mausrad über der Vorschau | – | Vorschau scrollt (nur wenn sie überläuft) |
| working | Klick „Stop“ | Esc | Abbruch, zurück zur Auswahl (nicht schließen), Sitzung bleibt |
| working | Mausrad über dem Text | ↑/↓, PgUp/PgDn, Space, Home/End | scrollen; wer am Ende steht, folgt dem Stream |
| result | Klick Primärknopf | Enter | Replace bzw. Copy answer |
| result | Klick „Copy“ (bei Ersetzen-Aktionen Sekundärknopf) | `c` | Ergebnis in CLIPBOARD, Panel schließt, Overlay meldet „Copied 342 characters“ |
| result | Klick „Copy“ an einem Codeblock | – | nur dieser Block in CLIPBOARD, Panel bleibt offen, Knopf zeigt 1,5 s „Copied“ |
| result | Klick „Back“ | BackSpace | zurück zur Auswahl derselben Markierung (Sitzung bleibt) |
| result | Klick „Show clean text“/„Show changes“ | `d` | Änderungen ein/aus (nur Ersetzen-Aktionen) |
| result | Mausrad, Klick/Ziehen in der Scrollleiste | ↑/↓, PgUp/PgDn, Space, Home/End | scrollen |
| error | Klick „Try again“ (wenn sinnvoll) | Enter | dieselbe Aktion erneut |
| error | Klick „Back“ | BackSpace | zurück zur Auswahl |
| error (leer, Daemon weg) | Klick „Close“ | Enter, Esc | schließen |

Statt `r` für Replace: Enter (Primärknopf). `r` bleibt im Ergebnis unbelegt, damit kein Buchstabe zwei Bedeutungen in
aufeinanderfolgenden Zuständen hat. `c` für Copy bleibt, weil die Totzeit (Grundsatz 3) das Doppel-`c` abfängt und `c`
in allen Anwendungen „copy“ bedeutet. Der Fußzeilen-Text entfällt zugunsten der Tasten-Chips in den Knöpfen.

### Technische Wechselwirkungen (durchdacht, in Xephyr zu bestätigen)

- **Fokus und Replace.** `awful.popup` ist ein Drawin, kein Client; ein Klick darauf ändert `client.focus` nicht und
  verschiebt keinen Eingabefokus. Replace prüft weiter `client.focus == A.client`; Klicks im Panel stören das nicht.
  Der Keygrabber greift nur die Tastatur, nicht den Zeiger, Klicks auf das Panel kommen also über die Widget-`buttons`
  an. Hover über `mouse::enter`/`mouse::leave` der Zeilen-Widgets, Zeiger `hand2` über `ui.popup.cursor` (Wibox-Eigenschaft)
  nur über klickbaren Elementen.
- **Klick außerhalb.** Kein Vollbild-Fänger: ein transparenter Vollbild-Wibox würde unter `awesome --no-argb` (derzeit
  aktiv) das Hintergrundbild statt der Fenster zeigen, ein `mousegrabber` würde den ganzen Zeiger stehlen und Hover/Rad
  selbst nachbauen müssen. Stattdessen drei Signale, alle im Modul, ohne Dotfile-Änderung:
  1. `client.connect_signal("button::press", …)`: jeder Klick in irgendein Fenster (auch ins Zielfenster: ein Klick dort
     hebt die Markierung auf oder setzt den Cursor, danach würde Replace einfügen statt ersetzen) schließt das Panel.
     `clientbuttons` bindet Taste 1 ohne Modifier, Awesome sieht den Klick also und reicht ihn weiter.
  2. `client.connect_signal("focus", …)`: Fokuswechsel weg von `A.client` (Alt-Tab-Modul, Tasklist, neues Fenster)
     schließt das Panel; ausgenommen das eigene Öffnen von `dictate-settings` (schließt vorher ohnehin).
  3. Desktop-Klick: rc.lua ruft bei Taste 1 auf dem Root `awful.util.dismiss_all_popups()`. Das Modul hängt sich davor,
     wie `continuity/notif_center.lua` es schon tut (Funktion umhüllen, Original aufrufen). Fehlt die Funktion, entfällt
     nur dieser Pfad.
  Klicks auf andere Wibar-Widgets schließen nicht (dort gibt es kein allgemeines Signal); das ist hinnehmbar, die Leiste
  verschiebt weder Fokus noch PRIMARY.
- **Mausrad** über `buttons` 4/5 am Text-Widget (`make_view:scroll(±3)`); Shift+Rad seitenweise. Scrollleiste: 4 px
  breite Pixel-Spur (Zellen wie der Pixel-Meter, gefüllt = sichtbarer Bereich, `gray-400` auf `px-off`), Klick springt,
  Ziehen über `mousegrabber` nur für die Dauer des Ziehens.
- **Text markieren oder ziehen im Ergebnis: bewusst nein.** Eine eigene Auswahl müsste PRIMARY übernehmen
  (`selection_acquire` in awesome-git). Folgen: die Anwendung mit der Original-Markierung verliert ihre Auswahl (GTK/Qt
  heben die Hervorhebung beim Verlust der Eigentümerschaft auf), Replace würde dann einfügen statt ersetzen, und
  `act grab` beim nächsten Mod+i läse den Panel-Text statt der Markierung des Users. Außerdem bliebe Antworttext in
  PRIMARY liegen, das jedes Programm ohne Rückfrage lesen kann; die Regel „nie CLIPBOARD lesen, nur auf Wunsch
  schreiben“ hätte ein Gegenstück ohne Wunsch. Ersatz: „Copy answer“ und „Copy“ je Codeblock (beides CLIPBOARD, über
  `act copy`; für Blöcke braucht es `act copy --id X --block N`, siehe Soll 7).
- **Sperre** (`lock.lua`) schließt wie heute; ein Klick-Pfad ändert daran nichts. Der Tastatur-Grab wird beim Schließen
  freigegeben, auch bei Light dismiss.

## 3. Öffnen per Maus

Bedingungen: PRIMARY muss erhalten bleiben (kein Klick in ein Textfeld), `client.focus` muss beim Öffnen das Zielfenster
sein (für Replace und `--app`).

| Option | PRIMARY | Fokus | Aufwand | Bewertung |
|---|---|---|---|---|
| **A. Symbol in der Wibar** (dictate-Pixel, Klick links = Aktionen, rechts = kleines Menü) | bleibt (Wibar ist ein Drawin, Klick markiert nichts) | bleibt (Wibar nimmt keinen Fokus) | klein; eine Zeile in `themes/qubit/theme.lua` | **empfohlen** |
| B. Eintrag im Hauptmenü (Rechtsklick auf den Desktop, `mymainmenu`) | bleibt | bleibt | klein; rc.lua | bei Tiling ist der Desktop selten frei; zwei Klicks; zusätzlich zu A sinnvoll, allein nicht |
| C. Mausgeste Mod+Mitteltaste auf ein Fenster | **gefährdet**: Awesome reicht Client-Klicks per `ReplayPointer` weiter, die Anwendung sähe einen Mittelklick und fügte PRIMARY ein | bleibt | rc.lua `clientbuttons` (Regel setzt `buttons`, ein Modul-Anhang würde überschrieben) | abgelehnt |
| D. Schwebendes Symbol neben jeder neuen Markierung (XFixes-Selection-Watcher) | bleibt | bleibt | groß | aufdringlich, reagiert auf jede Markierung im System; Inhalt würde zwar nicht gelesen, aber die Erwartung „Panel liest nur auf Wunsch“ verschwimmt. Kann, später, abschaltbar |
| E. Titelleisten-Knopf | bleibt | Klick auf Titelleiste aktiviert den Client (gewollt) | mittel | Titelleisten nur bei schwebenden Fenstern, deckt die Mehrheit nicht |

**Empfehlung A**, mit Rechtsklick-Menü als Erweiterung:
- Das Modul liefert `dictate.widget(s)` (neue Funktion, eigene Datei `awesome/dictate/widget.lua`): ein 8-px-Pixel im
  Stil des Logos, Ruhe `gray-400`, beim Diktat Teal (die Wellenform bleibt im Overlay), bei offenem Panel Teal mit
  Glühen. Linksklick: `actions.toggle()`. Rechtsklick: `awful.menu` mit „Text actions (Mod+i)“, „Dictate hands-free
  (Mod+Shift+e)“, „Settings (Mod+Ctrl+Shift+e)“. Mittelklick unbelegt. Tooltip: „Text actions on the selection (Mod+i)“.
- Das Modul kann sich **nicht sauber selbst** in die Leiste hängen: die Wibar baut `theme.at_screen_connect` aus einer
  Segmentliste (Powerline mit Farben je Segment, Bell vor der Uhr). Nachträgliches Einfügen in `s.mywibox` von außen
  wäre von der Struktur des Themes abhängig und bricht bei jeder Theme-Änderung. Nötig ist darum **eine Änderung im
  Theme** (`~/.config/awesome/themes/qubit/theme.lua`, chezmoi): ein Segment `{ widget = require("dictate").widget(s),
  color = palette.… }` vor der Bell. `rc.lua` selbst bleibt unverändert. Option B bräuchte zusätzlich `rc.lua`.
- **Screen beim Öffnen per Maus:** das Panel öffnet auf dem Screen des Zielfensters (dort wird ersetzt), aber nicht
  mehr screen-mittig, sondern über dem Zielfenster (siehe Muss 5). Ohne fokussiertes Fenster öffnet es auf dem Screen
  des Klicks. Ob es bei Mausöffnung immer beim Klick erscheinen soll, ist Frage 2.
- Der Klick auf das Symbol ändert weder Fokus noch PRIMARY, darum funktionieren `--app`, Replace und die
  Terminal-Regel unverändert. Klick auf das Symbol bei offenem Panel schließt (gleiches Umschalten wie Mod+i).

## 4. Empfehlungen nach Priorität

Aufwand: S bis 0,5 Tag, M bis 2 Tage, L mehr. „Sicherheit“ nennt Folgen für die Regeln aus `PLAN-actions.md`.

### Muss

| # | Was | Aufwand | Sicherheit |
|---|---|---|---|
| 1 | Maus im Panel: klickbare Zeilen und Knöpfe, Hover, ✕, Light dismiss (drei Signale aus 2.), Mausrad, Auslösen beim Loslassen | M | Light dismiss ruft `act clear` wie jedes Schließen; Cloud-Bestätigung bleibt „Wahl einer Aktion“, per Klick genauso explizit |
| 2 | Totzeit 300 ms und „neuer Tastendruck“ nach jedem Zustandswechsel; `r` im Ergebnis abschaffen, Primäraktion auf Enter je nach Ausgabe | S | verhindert unbeabsichtigtes Ersetzen bzw. Senden |
| 3 | Back und Try again ohne neues grab (gleiche Sitzungs-id, `act run` erneut) | S (Lua), Daemon prüfen: Ergebnis einer früheren run wird ersetzt, TTL bleibt 300 s ab grab | keine; die Markierung lebt schon jetzt bis TTL oder Schließen |
| 4 | Ergebnis-Knöpfe (Mockups 3a/3b) und Rückmeldung nach Copy/Replace im Overlay: „Copied 342 characters“, „Replacing… Mod+Ctrl+e stops“ mit Zeichenfortschritt, danach „Replaced“; prüfen, dass Mod+Ctrl+e die Tipp-Warteschlange wirklich leert | M | Overlay zeigt nur Zahlen, nie Text |
| 5 | Feste Position: Panel über dem Zielfenster zentriert (auf dessen Screen-Workarea begrenzt), Oberkante bleibt beim Wachsen stehen; Höhe wächst nur nach unten bis zur Grenze, nie Neu-Zentrieren während des Streams | S | keine |
| 6 | Wibar-Symbol `dictate.widget(s)` mit Links-/Rechtsklick (eine Zeile im Theme) | S | Klick liest PRIMARY wie Mod+i, also nur auf ausdrücklichen Wunsch |
| 7 | Leerzustand und Fehler als Anleitung (Mockups empty, error): „Nothing selected“ neutral (weiß, gestrichelter Slot), Satz was zu tun ist; Daemon fehlt: „Start it with systemctl --user start dictate“; Abbruch durch den User ist kein Fehler (zurück zur Auswahl) | S | keine |
| 8 | Klick in ein anderes Fenster oder Fokuswechsel schließt (P3/P4) | S | Tastatur-Grab endet sofort, kein „Hänger“ |

### Soll

| # | Was | Aufwand | Sicherheit |
|---|---|---|---|
| 1 | Aktionen gruppiert: „Answer in this panel“ / „Replace the selection“; Modell einmal je Gruppe, in der Zeile nur bei Abweichung | S | Cloud-Vorschau bleibt vollständig: Host oben, Modell je Gruppe, Hover-Zeile in der Fußzeile nennt Aktion und Zeichenzahl |
| 2 | Lesbare Namen für Modi: Label aus der Config (`[ai.modes]` bekommt optional `label`), Vorgabe „Clean up“, „To German“, … statt „Mode: x“ | S (Rust + GUI) | keine |
| 3 | Markdown-Teilmenge im Ergebnis: Absätze, Listen, `code`, Codeblöcke (Mono auf `black-extra`), fett/kursiv. Umwandlung im CLI/Daemon (Rust, testbar) in eine Segmentliste, das Panel baut daraus Pango-Markup mit Escaping je Segment. Links nicht klickbar | M | kein neuer Ausführungsweg: keine Links, keine Befehle; Text wird weiterhin nur angezeigt |
| 4 | Änderungen markiert bei Ersetzen-Aktionen (Mockup 3b): Wortdiff im Daemon (`act_run`-Antwort um Diff-Spannen erweitern), Schalter „Show clean text“; bei Translate Vorgabe aus, sonst an | M | Diff enthält nur Markierung und Ergebnis, die das Panel schon zeigt; keine Logs, nicht in `history.rs` |
| 5 | Arbeiten: Sekunden seit Start rechts im Kopf, Teal-Pixel als Schreibmarke am Ende des Streams, „Stop“-Knopf; nicht gestreamte Modi: „Rewriting 253 characters“ statt „Working…“ | S | keine |
| 6 | Scrollleiste als Pixel-Spur und Verlauf am unteren Rand statt „lines x to y of z“ | S | keine |
| 7 | Copy je Codeblock: `act copy --id X --block N` (Daemon schneidet den Block aus dem gespeicherten Ergebnis) | S | wie Copy: nur CLIPBOARD schreiben, nur auf Klick |
| 8 | Geheimnis-Sperre als eigener Block (Mockup secret): „Nothing was sent“, Arten als Liste, was man tun kann, „Open AI settings“; Aktionsliste entfällt ganz, wenn alles gesperrt ist | S | unverändert gesperrt; Text wird nicht gespeichert |
| 9 | Vorschau: höchstens 4 Zeilen plus „3 more lines“; Mausrad zeigt den Rest der 6-Zeilen-Vorschau des CLI | S | Vorschau bleibt auf 6 Zeilen / 400 Zeichen begrenzt |
| 10 | „Settings“ im Panel öffnet `dictate-settings --page actions` (gibt es schon) und schließt vorher | S | keine |

### Kann

| # | Was | Aufwand | Sicherheit |
|---|---|---|---|
| 1 | Zeilen der Geheimnisse nennen („line 3“, Mockup secret): Daemon liefert Zeilennummern je Fund | S | nur Positionen, kein Inhalt; nicht loggen |
| 2 | Hauptmenü-Eintrag „Text actions“ (Option B) | S | wie A |
| 3 | Panel neben dem Mauszeiger, wenn per Maus geöffnet und der Zeiger über dem Zielfenster liegt | S | keine |
| 4 | Schwebendes Symbol nach einer Markierung (Option D), abschaltbar, Vorgabe aus | L | Selection-Watcher sieht nur Eigentümerwechsel, liest nichts; trotzdem ausdrücklich dokumentieren |
| 5 | Zuletzt benutzte Aktion zuerst aktiv (Enter genügt) | S | bei Cloud bleibt Enter eine bewusste Wahl, weil die Zeile sichtbar aktiv ist; Totzeit gilt |

## 5. Gestaltung (für die Umsetzung)

- Kopf: Label des Zustands (Großbuchstaben, 12 px, 0,2 em, `gray-400`) links, ✕ rechts (32 × 32 Trefferfläche,
  Hover `black-extra`, Strich weiß). Keine Metadaten-Zeile mit Mittelpunkten mehr.
- Darunter ein Satz zur Quelle („253 characters, 7 lines from Firefox“; WM_CLASS ist schon bekannt) und ein Satz zum
  Ziel mit Teal-Pixel bei Cloud („Sends to **api.anthropic.com** when you pick an action“), lokal `gray-400`-Pixel
  („Runs on **localhost:11434**“).
- Zeilen 40 px (Trefferfläche), Hover/aktiv wie Seitenleiste (`black-extra` + 7-px-Teal-Pixel), gesperrt: nicht gedimmt
  in der Liste, sondern Liste weg und Sperrblock (Soll 8); bei Teil-Sperre (lokal erlaubt, Regel `block`) Zeile mit
  „Blocked“ rechts statt Modell.
- Knöpfe nach DESIGN.md: Primär Teal mit schwarzem Text und Glühen, Sekundär 1 px `gray-800`, Ghost ohne Rahmen für
  Back/Settings; jeder Knopf trägt seinen Tasten-Chip (Mono, 1 px Rahmen).
- Höhen: Panel wächst nur nach unten; Ergebnis höchstens `actions_view_lines`, dann Scrollleiste.
- Keine Animation außer der Antwort auf eine Handlung (Hover sofort, Copy-Bestätigung 1,5 s).

## Offene Fragen an den User

1. **Klick außerhalb schließt immer**, auch bei einem fertigen, langen Ergebnis (Ergebnis ist dann weg, die Sitzung
   wird gelöscht)? Alternative: im Ergebniszustand schließt nur ✕/Esc, ein Klick daneben nicht. Empfehlung: immer
   schließen (einheitlich, Tastatur-Grab endet sofort).
2. **Screen bei Öffnen per Maus:** Panel über dem Zielfenster (wo ersetzt wird) oder auf dem Screen des geklickten
   Leistensymbols? Empfehlung: über dem Zielfenster.
3. **Wibar-Symbol im Theme** (`themes/qubit/theme.lua`, chezmoi) als eigenes Powerline-Segment vor der Bell: einverstanden,
   und welche Segmentfarbe? Zusätzlich ein Eintrag im Rechtsklick-Hauptmenü (rc.lua)?
4. **Änderungen markieren** bei Ersetzen-Aktionen standardmäßig an (außer Translate) oder aus?

## Entscheidungen des Users (23.10.2026)

1. Klick neben das Panel schließt immer, auch ein fertiges Ergebnis.
2. Das Panel erscheint über dem Zielfenster (Bildschirm des Fensters mit der Markierung), auch wenn es per Leistensymbol
   geöffnet wurde.
3. Öffnen per Maus: Symbol in der unteren Leiste (eigenes Segment im Theme `themes/qubit/theme.lua`, chezmoi, Teal als
   einziger Akzent) **und** Eintrag „Text actions“ im Rechtsklick-Hauptmenü von Awesome (`rc.lua`, chezmoi).
4. Ersetzen-Aktionen zeigen die Änderungen standardmäßig markiert, außer Translate; „Show clean text“ schaltet um.
5. Neue Aktion „Refine prompt“ (Taste p, Ersetzen) und Diktat-Modus `prompt`: Sprache wie die Eingabe, Umfang
   strukturiert-mittel (Ziel, Kontext, Anforderungen, Ausgabeformat, offene Punkte als kurze Liste; etwa 2–4-fache
   Länge), nichts erfinden, nicht ausführen. Umgesetzt (23.10.2026): `p` kollidiert weder im Auswahlzustand (e, t, r,
   a, c, 1–9) noch im Ergebnis (Enter, c, r, j/k und Blättertasten); im Ergebnis bleibt `p` ohne Wirkung. Der
   unveränderte Modus `prompt` steht nicht zusätzlich auf einer Zifferntaste. Für Punkt 4: Refine prompt wie Translate
   ohne markierte Änderungen (der Text ist ganz neu). Replace tippt Zeilenumbrüche als Return; in Chat-Feldern, in
   denen Enter sendet, ist Copy der Weg (siehe `SPEC-v2.md`).
