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

//! Schrift des Einstellungsfensters aus den GTK-Einstellungen, wie `dictate-ui` sie über GTK 4 bekommt (Lesart L41,
//! Messpunkt M1 samt Nachmessung in PLAN-wayland.md 6h; Entscheidung 21).
//!
//! [`gtk_font`] ist die einzige Quelle: gemessen ist nur, was GTK 4 im verschachtelten niri tut (außerhalb einer
//! Sandbox nur `settings.ini`, kein gsettings, kein Portal; live auf blub abgeleitet). Zeigt der Live-Test etwas
//! anderes, ändert sich nur diese Funktion.
//!
//! `gtk-font-name` ist eine Pango-Beschreibung („Roboto 12“, „Noto Sans SemiBold 11“, „Sans 16px“): das letzte Wort
//! ist die Größe, Stil- und Gewichtswörter davor fallen weg (GTK 4 übernimmt nur Familie und Größe, gemessen), und nur
//! die Familie geht an `fc-match` (die ganze Zeichenkette läse fontconfig als Familie „Roboto 12“, der Messfehler von
//! M1). Punkte werden wie bei GTK zu logischen Pixeln: pt × dpi / 72, dpi aus `gtk-xft-dpi` / 1024, sonst 96.
//! Was fontconfig als Ersatz liefert, gilt ohne Prüfung des Dateinamens (GTK zeigt denselben Ersatz). Jede Datei wird
//! vor dem Laden mit skrifa geprüft, weil egui an einer unlesbaren Datei mit panic abbricht.

use egui::epaint::text::{FontTweak, VariationCoords};
use egui::FontData;
use std::path::{Path, PathBuf};

/// Vorgabe von GTK 4 ohne `settings.ini` oder ohne Schlüssel (gemessen, M1)
pub const GTK_DEFAULT: &str = "Adwaita Sans 11";
/// Auflösung ohne `gtk-xft-dpi` (gemessen: 12 pt → 16 px)
const DEFAULT_DPI: f64 = 96.0;
/// Grenzen der Basisgröße in logischen Pixeln: die Datei ist fremde Eingabe (Regel 4), das Fenster hat feste Größe
const MIN_PX: f32 = 8.0;
const MAX_PX: f32 = 32.0;
/// Größte gelesene Datei (`settings.ini`), wie bei der Palette
const MAX_INI: u64 = 64 * 1024;
/// Größte Schriftdatei (Sammlungen wie Noto CJK haben rund 20 MiB)
const MAX_FONT: u64 = 64 * 1024 * 1024;

/// Familie und Größe der Basisschrift
#[derive(Debug, Clone, PartialEq)]
pub struct Spec {
    /// Familienliste wie in der Pango-Beschreibung (meist ein Eintrag)
    pub families: Vec<String>,
    /// Größe in logischen Pixeln
    pub px: f32,
    /// Herkunft fürs Log (Pfad oder Vorgabe, mit Grund)
    pub origin: String,
}

/// Die Schrift, die GTK 4 zeigt: `gtk-font-name` (und `gtk-xft-dpi`) aus `$XDG_CONFIG_HOME/gtk-4.0/settings.ini`,
/// sonst `~/.config/gtk-4.0/settings.ini`; fehlt Datei oder Schlüssel, `Adwaita Sans 11`
pub fn gtk_font() -> Spec {
    let Some(path) = settings_ini_path() else {
        return default_spec("no XDG_CONFIG_HOME and no HOME".into());
    };
    let text = match crate::ui::fsutil::read_regular(&path, MAX_INI).and_then(|b| String::from_utf8(b).map_err(|_| format!("{} is not UTF-8", path.display())))
    {
        Ok(t) => t,
        Err(e) => return default_spec(e),
    };
    let ini = parse_ini(&text);
    let Some(name) = ini.font_name else {
        return default_spec(format!("{}: no gtk-font-name", path.display()));
    };
    let dpi = ini.xft_dpi.filter(|d| *d > 0).map(|d| d as f64 / 1024.0).unwrap_or(DEFAULT_DPI);
    let (families, px) = describe(&name, dpi);
    Spec { families, px, origin: path.display().to_string() }
}

fn default_spec(why: String) -> Spec {
    let (families, px) = describe(GTK_DEFAULT, DEFAULT_DPI);
    Spec { families, px, origin: format!("GTK default ({why})") }
}

fn settings_ini_path() -> Option<PathBuf> {
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|p| !p.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").filter(|p| !p.is_empty()).map(|h| PathBuf::from(h).join(".config")))?;
    Some(config.join("gtk-4.0/settings.ini"))
}

/// Die Schlüssel aus der Gruppe `[Settings]`, die das Fenster liest
#[derive(Debug, Default, PartialEq)]
pub struct Ini {
    pub font_name: Option<String>,
    pub xft_dpi: Option<i64>,
}

/// `settings.ini` im Format von GKeyFile: Gruppen in eckigen Klammern, `#`-Kommentare, `Schlüssel = Wert` (Leerraum um
/// `=` zählt nicht), Escapes `\s \n \t \r \\`; der letzte Eintrag eines Schlüssels gilt, übersetzte Schlüssel
/// (`gtk-font-name[de]`) und andere Gruppen zählen nicht
pub fn parse_ini(text: &str) -> Ini {
    let mut ini = Ini::default();
    let mut in_settings = false;
    for line in text.lines() {
        let line = line.trim_start();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(group) = line.strip_prefix('[') {
            in_settings = group.trim_end().strip_suffix(']') == Some("Settings");
            continue;
        }
        if !in_settings {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else { continue };
        let value = unescape(value.trim_start());
        match key.trim_end() {
            "gtk-font-name" => ini.font_name = Some(value),
            "gtk-xft-dpi" => ini.xft_dpi = value.trim().parse().ok(),
            _ => {}
        }
    }
    ini
}

fn unescape(v: &str) -> String {
    let mut out = String::with_capacity(v.len());
    let mut it = v.chars();
    while let Some(c) = it.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match it.next() {
            Some('s') => out.push(' '),
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('\\') => out.push('\\'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

/// Wörter, die Pango in einer Beschreibung als Stil liest (Gewicht, Stil, Variante, Dehnung, Lage); verglichen ohne
/// Bindestrich und ohne Groß/klein, wie `field_matches` in Pango
const STYLE_WORDS: &[&str] = &[
    "normal",
    "roman",
    "oblique",
    "italic",
    "smallcaps",
    "allsmallcaps",
    "petitecaps",
    "allpetitecaps",
    "unicase",
    "titlecaps",
    "thin",
    "ultralight",
    "extralight",
    "light",
    "semilight",
    "demilight",
    "book",
    "regular",
    "medium",
    "semibold",
    "demibold",
    "bold",
    "ultrabold",
    "extrabold",
    "heavy",
    "black",
    "ultrablack",
    "extrablack",
    "ultraheavy",
    "ultracondensed",
    "extracondensed",
    "condensed",
    "semicondensed",
    "semiexpanded",
    "expanded",
    "extraexpanded",
    "ultraexpanded",
    "notrotated",
    "south",
    "upsidedown",
    "north",
    "rotatedleft",
    "east",
    "rotatedright",
    "west",
];

fn is_style_word(w: &str) -> bool {
    let w: String = w.chars().filter(|c| *c != '-').flat_map(char::to_lowercase).collect();
    STYLE_WORDS.contains(&w.as_str())
}

/// Größe als Wort: `12`, `10.5`, `16px`; Punkte oder Pixel
enum Size {
    Points(f64),
    Pixels(f64),
}

fn size_word(w: &str) -> Option<Size> {
    let (num, px) = match w.strip_suffix("px") {
        Some(n) => (n, true),
        None => (w, false),
    };
    if num.is_empty() || !num.bytes().all(|b| b.is_ascii_digit() || b == b'.') {
        return None;
    }
    let v: f64 = num.parse().ok()?;
    Some(if px { Size::Pixels(v) } else { Size::Points(v) })
}

/// Pango-Beschreibung → (Familienliste, Größe in logischen Pixeln). Von hinten: Varianten (`@wght=…`) und Merkmale
/// (`#…`) fallen weg, das letzte Wort ist die Größe, Stilwörter davor fallen weg, der Rest ist die Familienliste
/// (Komma getrennt). Ohne Familie gilt die von [`GTK_DEFAULT`], ohne Größe 11 pt (Vorgabe von GTK); beides ist nicht
/// gemessen. Die Größe wird auf [`MIN_PX`]..[`MAX_PX`] begrenzt.
pub fn describe(desc: &str, dpi: f64) -> (Vec<String>, f32) {
    let mut words: Vec<&str> = desc.split_whitespace().collect();
    while words.last().is_some_and(|w| w.starts_with('@') || w.starts_with('#')) {
        words.pop();
    }
    let size = words.last().and_then(|w| size_word(w));
    if size.is_some() {
        words.pop();
    }
    while words.last().is_some_and(|w| is_style_word(w)) {
        words.pop();
    }
    let mut families: Vec<String> = words.join(" ").split(',').map(|f| f.trim().to_string()).filter(|f| !f.is_empty()).collect();
    if families.is_empty() {
        families = vec![GTK_DEFAULT.rsplit_once(' ').map_or(GTK_DEFAULT, |(f, _)| f).to_string()];
    }
    let px = match size {
        Some(Size::Pixels(px)) => px,
        Some(Size::Points(pt)) => pt * dpi / 72.0,
        None => 11.0 * dpi / 72.0,
    };
    (families, (px as f32).clamp(MIN_PX, MAX_PX))
}

/// Eine Fläche in einer Datei, wie fontconfig sie meldet
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Face {
    pub path: PathBuf,
    /// `%{index}`: untere 16 Bit die Fläche einer Sammlung (`.ttc`), obere die benannte Instanz einer variablen Datei
    pub index: u32,
}

impl Face {
    fn face_index(&self) -> u32 {
        self.index & 0xffff
    }

    fn named_instance(&self) -> bool {
        self.index >> 16 != 0
    }
}

/// Muster für fontconfig: Familien mit Komma, Sonderzeichen der Namenssyntax maskiert
pub fn pattern(families: &[String], weight: Option<&str>) -> String {
    let esc = |f: &str| {
        let mut s = String::with_capacity(f.len());
        for c in f.chars() {
            if matches!(c, '\\' | '-' | ':' | ',') {
                s.push('\\');
            }
            s.push(c);
        }
        s
    };
    let mut p = families.iter().map(|f| esc(f)).collect::<Vec<_>>().join(",");
    if let Some(w) = weight {
        p.push_str(":weight=");
        p.push_str(w);
    }
    p
}

/// Gesamtfrist aller `fc-match`-Aufrufe beim Start (Entscheidung 22): großzügig, weil fontconfig bei kaltem Cache (nach
/// einer Schrift-Installation) einige Sekunden braucht; danach gilt der Rückfall ohne fontconfig ([`fallback_face`]),
/// und das Fenster geht trotzdem auf
pub const FC_DEADLINE: std::time::Duration = std::time::Duration::from_secs(10);
/// Größte gelesene Ausgabe je Aufruf (eine Zeile Pfad und eine Zahl)
const FC_MAX_OUT: u64 = 64 * 1024;

/// Flächen über `fc-match` auflösen (immer irgendeine Schrift: fontconfig liefert den Ersatz, den auch GTK zeigt). Alle
/// Aufrufe laufen gleichzeitig: jeder lädt Konfiguration und Cache von fontconfig neu (auf blub rund 115 ms je Aufruf,
/// gemessen), nacheinander verlängerten sie den Start um ein Vielfaches (Messpunkt M4). Antwortet ein Aufruf nicht binnen
/// [`FC_DEADLINE`], wird er beendet und eingesammelt (kein Zombie), sein Ergebnis ist ein Fehler.
pub fn resolve_all(patterns: &[&str]) -> Vec<Result<Face, String>> {
    resolve_with(Path::new("fc-match"), patterns, FC_DEADLINE)
}

fn resolve_with(program: &Path, patterns: &[&str], limit: std::time::Duration) -> Vec<Result<Face, String>> {
    use std::io::Read;
    use std::process::{Child, Command, ExitStatus, Stdio};
    let started = std::time::Instant::now();
    let mut children: Vec<Result<Child, String>> = patterns
        .iter()
        .map(|p| {
            Command::new(program)
                .args(["-f", "%{file}\n%{index}\n", "--", p])
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|e| format!("fc-match: {e}"))
        })
        .collect();
    let mut ended: Vec<Option<Result<ExitStatus, String>>> = vec![None; children.len()];
    loop {
        let mut running = false;
        for (child, end) in children.iter_mut().zip(ended.iter_mut()).filter(|(_, e)| e.is_none()) {
            match child {
                Err(e) => *end = Some(Err(e.clone())),
                Ok(c) => match c.try_wait() {
                    Ok(Some(status)) => *end = Some(Ok(status)),
                    Ok(None) => running = true,
                    Err(e) => *end = Some(Err(format!("fc-match: {e}"))),
                },
            }
        }
        if !running {
            break;
        }
        if started.elapsed() >= limit {
            for (child, end) in children.iter_mut().zip(ended.iter_mut()).filter(|(_, e)| e.is_none()) {
                if let Ok(c) = child {
                    let _ = c.kill();
                    let _ = c.wait();
                }
                *end = Some(Err(format!("fc-match gave no answer within {} s", limit.as_secs_f32())));
            }
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    children
        .into_iter()
        .zip(ended)
        .zip(patterns)
        .map(|((child, end), p)| {
            let status = end.unwrap_or_else(|| Err("fc-match: not waited for".into()))?;
            if !status.success() {
                return Err(format!("fc-match failed for {p:?}"));
            }
            let mut out = Vec::new();
            if let Some(stdout) = child.ok().and_then(|mut c| c.stdout.take()) {
                stdout.take(FC_MAX_OUT).read_to_end(&mut out).map_err(|e| format!("fc-match: {e}"))?;
            }
            parse_match(&String::from_utf8_lossy(&out)).ok_or_else(|| format!("fc-match gave no file for {p:?}"))
        })
        .collect()
}

fn parse_match(out: &str) -> Option<Face> {
    let mut lines = out.lines();
    let path = lines.next().filter(|p| p.starts_with('/'))?;
    let index = lines.next().and_then(|i| i.trim().parse().ok()).unwrap_or(0);
    Some(Face { path: PathBuf::from(path), index })
}

/// Datei lesen, mit skrifa prüfen, Gewicht setzen. Variable Dateien (benannte Instanz von fontconfig oder Achse
/// `wght`) bekommen `weight` über die Achse; eine statische Datei hat ihr Gewicht schon (fontconfig wählt dann die
/// passende Datei, etwa `Roboto-Bold.ttf` für 600, wenn es keine SemiBold gibt)
pub fn load(face: &Face, weight: f32) -> Result<FontData, String> {
    let bytes = crate::ui::fsutil::read_regular(&face.path, MAX_FONT)?;
    data(bytes, face, weight)
}

fn data(bytes: Vec<u8>, face: &Face, weight: f32) -> Result<FontData, String> {
    let index = face.face_index();
    if let Err(e) = skrifa::FontRef::from_index(&bytes, index) {
        return Err(format!("{} (face {index}) is not a font egui can read: {e}", face.path.display()));
    }
    let mut data = FontData::from_owned(bytes);
    data.index = index;
    let variable = face.named_instance() || data.variation_axes().iter().any(|a| a.tag == egui::epaint::text::Tag::new(b"wght"));
    if variable {
        data = data.tweak(FontTweak { coords: VariationCoords::new([(b"wght", weight)]), ..Default::default() });
    }
    Ok(data)
}

/// Rückfall ohne fontconfig, falls kein Aufruf von `fc-match` eine Basisschrift lieferte (Frist abgelaufen, fontconfig
/// fehlt): seit U2 (PLAN-lizenz.md) bringt egui keine eigene Schrift mehr mit (`default_fonts` aus). Dateinamen
/// verbreiteter Sans-Schriften in dieser Reihenfolge; der erste Name, der unter einem der Ordner liegt, gilt
pub const FALLBACK_FILES: &[&str] =
    &["AdwaitaSans-Regular.ttf", "DejaVuSans.ttf", "NotoSans-Regular.ttf", "LiberationSans-Regular.ttf", "Cantarell-VF.otf", "Roboto-Regular.ttf"];
/// Höchstens so viele Einträge durchsucht [`fallback_face`] (Schutz vor sehr großen oder zyklischen Bäumen)
const FALLBACK_MAX_ENTRIES: usize = 50_000;
/// Höchste Ordnertiefe unter einem Schriftordner (Arch: `/usr/share/fonts/TTF/x.ttf`, Debian: `…/truetype/dejavu/x.ttf`)
const FALLBACK_MAX_DEPTH: usize = 4;

/// Schriftordner des Systems und des Users für [`fallback_face`] (wie die Vorgabe von fontconfig)
pub fn fallback_dirs() -> Vec<PathBuf> {
    let mut dirs = vec![PathBuf::from("/usr/share/fonts"), PathBuf::from("/usr/local/share/fonts")];
    if let Some(d) = dirs::data_dir() {
        dirs.push(d.join("fonts"));
    }
    dirs
}

/// Erste Datei aus [`FALLBACK_FILES`] unter `dirs` (Reihenfolge der Liste vor der Reihenfolge der Ordner); Verweise
/// werden nicht verfolgt, ein unlesbarer Ordner fehlt nur
pub fn fallback_face(dirs: &[PathBuf]) -> Option<Face> {
    let mut found: Vec<Option<PathBuf>> = vec![None; FALLBACK_FILES.len()];
    let mut stack: Vec<(PathBuf, usize)> = dirs.iter().map(|d| (d.clone(), 0)).collect();
    stack.reverse();
    let mut seen = 0usize;
    while let Some((dir, depth)) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        let mut entries: Vec<_> = rd.flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        let mut sub = Vec::new();
        for e in entries {
            seen += 1;
            if seen > FALLBACK_MAX_ENTRIES {
                break;
            }
            let Ok(ty) = e.file_type() else { continue };
            if ty.is_dir() && depth < FALLBACK_MAX_DEPTH {
                sub.push((e.path(), depth + 1));
            } else if ty.is_file() {
                let name = e.file_name();
                if let Some(i) = FALLBACK_FILES.iter().position(|f| name.as_os_str() == std::ffi::OsStr::new(f)) {
                    found[i].get_or_insert_with(|| e.path());
                }
            }
        }
        stack.extend(sub.into_iter().rev());
    }
    found.into_iter().flatten().next().map(|path| Face { path, index: 0 })
}

/// Name einer Datei fürs Log (ohne Ordner)
pub fn file_name(p: &Path) -> String {
    p.file_name().map_or_else(|| p.display().to_string(), |n| n.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {

    /// Entscheidung 22: ein hängendes `fc-match` hält das Fenster nicht auf; es wird beendet, die anderen Aufrufe gelten
    #[test]
    fn a_hanging_fc_match_is_stopped_at_the_deadline() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("dictate-fc-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let fake = dir.join("fc-match");
        // antwortet für "fast", hängt für alles andere (exec: der Prozess selbst schläft, nichts bleibt übrig)
        std::fs::write(&fake, "#!/bin/sh\nif [ \"$4\" = fast ]; then printf '/fonts/a.ttf\\n2\\n'; exit 0; fi\nexec sleep 30\n").unwrap();
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        let t = std::time::Instant::now();
        let r = resolve_with(&fake, &["fast", "slow"], std::time::Duration::from_millis(300));
        let took = t.elapsed();
        assert!(took < std::time::Duration::from_secs(3), "nach der Frist beendet ({took:?})");
        assert_eq!(r[0].as_ref().unwrap().path, PathBuf::from("/fonts/a.ttf"));
        assert_eq!(r[0].as_ref().unwrap().index, 2);
        assert!(r[1].as_ref().unwrap_err().contains("no answer within"), "{:?}", r[1].as_ref().err());
        // ein fehlendes Programm ist ein Fehler je Muster, kein Absturz
        let r = resolve_with(&dir.join("missing"), &["x"], std::time::Duration::from_millis(300));
        assert!(r[0].is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
    use super::*;

    fn fams(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    /// Entscheidung 21: die Größe wird abgetrennt, nur die Familie geht an fc-match (nie „Roboto 12“)
    #[test]
    fn pango_description_like_gtk() {
        assert_eq!(describe("Roboto 12", 96.0), (fams(&["Roboto"]), 16.0));
        assert_eq!(pattern(&describe("Roboto 12", 96.0).0, None), "Roboto");
        // gemessen (Nachmessung M1): 120 dpi aus gtk-xft-dpi, px direkt, Adwaita Sans 11 = 14,667 px
        assert_eq!(describe("Noto Sans 12", 120.0), (fams(&["Noto Sans"]), 20.0));
        assert_eq!(describe("Noto Sans 16px", 96.0), (fams(&["Noto Sans"]), 16.0));
        let (f, px) = describe(GTK_DEFAULT, 96.0);
        assert_eq!(f, fams(&["Adwaita Sans"]));
        assert!((px - 14.667).abs() < 0.001);
        // Stilwörter fallen weg (GTK 4 übernimmt das Gewicht nicht, gemessen), mit und ohne Bindestrich
        assert_eq!(describe("Noto Sans SemiBold 11", 96.0).0, fams(&["Noto Sans"]));
        assert_eq!(describe("Cantarell Semi-Bold Italic 11", 96.0).0, fams(&["Cantarell"]));
        assert_eq!(describe("Source Sans 3 Bold 10.5", 72.0), (fams(&["Source Sans 3"]), 10.5));
        // Familienliste, Varianten, fehlende Größe, fehlende Familie, Grenzen
        assert_eq!(describe("Inter, Noto Sans 11 @wght=500", 96.0).0, fams(&["Inter", "Noto Sans"]));
        assert_eq!(describe("Roboto", 96.0), (fams(&["Roboto"]), 11.0 * 96.0 / 72.0));
        assert_eq!(describe("12", 96.0), (fams(&["Adwaita Sans"]), 16.0));
        assert_eq!(describe("", 96.0).0, fams(&["Adwaita Sans"]));
        assert_eq!(describe("Roboto 200", 96.0).1, MAX_PX);
        assert_eq!(describe("Roboto 1px", 96.0).1, MIN_PX);
        // eine Familie, deren letzter Teil wie ein Stilwort aussieht, verliert ihn auch bei Pango
        assert_eq!(describe("Roboto Light 12", 96.0).0, fams(&["Roboto"]));
    }

    #[test]
    fn fontconfig_pattern_is_escaped() {
        assert_eq!(pattern(&fams(&["Noto Sans"]), Some("semibold")), "Noto Sans:weight=semibold");
        assert_eq!(pattern(&fams(&["A-B", "C:D", "E\\F"]), None), "A\\-B,C\\:D,E\\\\F");
        assert_eq!(pattern(&fams(&["-x"]), None), "\\-x");
    }

    #[test]
    fn settings_ini_like_gkeyfile() {
        let ini =
            parse_ini("# Kommentar\n[Other]\ngtk-font-name=Wrong 9\n[Settings]\ngtk-font-name[de]=Nope 8\n  gtk-font-name = Roboto 12\ngtk-xft-dpi=122880\n");
        assert_eq!(ini, Ini { font_name: Some("Roboto 12".into()), xft_dpi: Some(122880) });
        assert_eq!(parse_ini("[Settings]\ngtk-font-name=A 9\ngtk-font-name=B\\sC 10\n").font_name.as_deref(), Some("B C 10"));
        assert_eq!(parse_ini("gtk-font-name=Outside 9\n").font_name, None);
        assert_eq!(parse_ini("[Settings]\ngtk-xft-dpi=-1\n").xft_dpi, Some(-1));
        assert_eq!(parse_ini("[Settings]\ngtk-xft-dpi=x\n").xft_dpi, None);
    }

    #[test]
    fn fc_match_output_and_index() {
        assert_eq!(parse_match("/usr/share/fonts/TTF/Roboto-Bold.ttf\n0\n"), Some(Face { path: "/usr/share/fonts/TTF/Roboto-Bold.ttf".into(), index: 0 }));
        // benannte Instanz einer variablen Datei (Adwaita Sans SemiBold: 6 << 16)
        let f = parse_match("/usr/share/fonts/Adwaita/AdwaitaSans-Regular.ttf\n393216\n").unwrap();
        assert!(f.named_instance());
        assert_eq!(f.face_index(), 0);
        assert_eq!(parse_match("\n0\n"), None);
        assert_eq!(parse_match("relative.ttf\n0\n"), None);
    }

    /// egui bräche an einer unlesbaren Datei mit panic ab: solche Dateien werden vorher abgelehnt
    #[test]
    fn unreadable_font_is_rejected() {
        let face = Face { path: "/x/broken.ttf".into(), index: 0 };
        assert!(data(b"not a font at all".to_vec(), &face, 400.0).unwrap_err().contains("not a font"));
        assert!(data(Vec::new(), &face, 400.0).is_err());
        // eine echte Datei aus egui selbst wird angenommen, Fläche 1 einer Einzeldatei nicht
        let ok = epaint_default_font();
        assert!(data(ok.clone(), &face, 400.0).is_ok());
        assert!(data(ok, &Face { path: face.path.clone(), index: 1 }, 400.0).is_err());
    }

    /// Echte Schriftdatei für Tests: Hack aus `epaint_default_fonts`, nur als Test-Abhängigkeit (das Binary ist ohne
    /// `default_fonts`, U2)
    fn epaint_default_font() -> Vec<u8> {
        epaint_default_fonts::HACK_REGULAR.to_vec()
    }

    /// U2: egui bringt im Binary keine eigene Schrift mit; käme `default_fonts` über eine Abhängigkeit zurück, stünden
    /// Hack und Ubuntu Light wieder im GPL-Binary (Lizenzfrage, PLAN-lizenz.md U2)
    #[test]
    fn egui_brings_no_fonts_of_its_own() {
        assert!(egui::FontDefinitions::default().font_data.is_empty());
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("dictate-fallback-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Rückfall ohne fontconfig: die Liste entscheidet vor dem Ordner, Unterordner werden durchsucht, andere Dateien
    /// und fehlende Ordner stören nicht, die gefundene Datei lädt
    #[test]
    fn fallback_finds_the_preferred_file_without_fontconfig() {
        let a = temp_dir("a");
        let b = temp_dir("b");
        std::fs::create_dir_all(a.join("TTF")).unwrap();
        std::fs::create_dir_all(b.join("truetype/dejavu")).unwrap();
        std::fs::write(a.join("TTF/Roboto-Regular.ttf"), epaint_default_font()).unwrap();
        std::fs::write(a.join("TTF/Other.ttf"), b"x").unwrap();
        std::fs::write(b.join("truetype/dejavu/DejaVuSans.ttf"), epaint_default_font()).unwrap();
        let dirs = [a.join("missing"), a.clone(), b.clone()];
        let face = fallback_face(&dirs).expect("found");
        assert_eq!(face.path, b.join("truetype/dejavu/DejaVuSans.ttf"));
        assert!(load(&face, 400.0).is_ok());
        assert_eq!(fallback_face(std::slice::from_ref(&a)).unwrap().path, a.join("TTF/Roboto-Regular.ttf"));
        assert!(fallback_face(&[a.join("missing")]).is_none());
        let _ = std::fs::remove_dir_all(&a);
        let _ = std::fs::remove_dir_all(&b);
    }

    /// Tiefer als [`FALLBACK_MAX_DEPTH`] wird nicht gesucht
    #[test]
    fn fallback_stops_at_the_depth_limit() {
        let a = temp_dir("deep");
        let deep = a.join("1/2/3/4/5");
        std::fs::create_dir_all(&deep).unwrap();
        std::fs::write(deep.join("DejaVuSans.ttf"), epaint_default_font()).unwrap();
        assert!(fallback_face(std::slice::from_ref(&a)).is_none());
        std::fs::write(a.join("1/2/3/4/DejaVuSans.ttf"), epaint_default_font()).unwrap();
        assert_eq!(fallback_face(std::slice::from_ref(&a)).unwrap().path, a.join("1/2/3/4/DejaVuSans.ttf"));
        let _ = std::fs::remove_dir_all(&a);
    }
}
