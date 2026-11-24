# Wortlisten

## eff_large_wordlist.txt

EFF Large Wordlist for Passphrases: 7776 Wörter, eine Zeile je Wort, Format `NNNNN<TAB>wort` (fünf Würfelzahlen 1 bis 6).
Unverändert übernommen, nicht neu formatiert.

| | |
|---|---|
| Quelle | https://www.eff.org/files/2016/07/18/eff_large_wordlist.txt (Seite: https://www.eff.org/dice, Liste von Joseph Bonneau für die EFF) |
| Abgerufen | 2026-10-23 |
| SHA-256 | `addd35536511597a02fa0a9ff1e5284677b8883b83e986e43f15a3db996b903e` |
| Lizenz | Creative Commons Attribution 4.0 International (CC BY 4.0, https://creativecommons.org/licenses/by/4.0/). Die Copyright-Seite der EFF (https://www.eff.org/copyright) stellt alles eigene Material der Website unter CC BY 4.0 (Text seit Anfang 2025); bei Veröffentlichung der Liste im Juli 2016 nannte sie CC BY 3.0 US. Festgelegt in O1 (`PLAN-lizenz.md` U3d). |

Namensnennung: „EFF Large Wordlist for Passphrases“, Electronic Frontier Foundation, https://www.eff.org/dice, unverändert, CC BY 4.0
(https://creativecommons.org/licenses/by/4.0/).

Verwendung: `src/actions/wordlist.rs` bindet die Datei mit `include_str!` ins Binary ein; `src/actions/secrets.rs` erkennt damit
Passphrasen aus den Generatoren von Bitwarden und Vaultwarden, die genau diese Liste nutzen (Geheimnisfilter für Textaktionen,
Regel 8c). Die Liste verlässt den Rechner nie. Ein Test (`wordlist::tests::file_is_the_unmodified_eff_large_list`) vergleicht
die Prüfsumme; wird die Datei bewusst ersetzt, Prüfsumme hier und im Test ändern und die Fehlalarm-Messung aus der Moduldoku
von `secrets.rs` wiederholen.

Prüfen: `sha256sum assets/wordlists/eff_large_wordlist.txt`
