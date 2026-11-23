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

//! Pseudonymisieren persönlicher Daten im Diagnosebericht (`PLAN-diagnose.md`, Antwort 4): gleiche Werte bekommen
//! innerhalb einer Sitzung gleiche Platzhalter (`user-1`, `host-2`, `ip-3`, `mac-1`, `ssid-1`, `path-4`, `email-1`, `id-1`),
//! Zusammenhänge
//! bleiben erhalten. Die Zuordnung lebt nur in [`Pseudonyms`] (Speicher der Sitzung im Daemon), wird nie geloggt (das
//! `Debug` zeigt nur Zahlen) und nie gesendet; [`Pseudonyms::restore`] übersetzt Platzhalter in Antworten zurück (nur
//! für die Anzeige im Panel). Mit `enabled = false` (Schalter, Config folgt in Aufgabe 7/10) bleibt der Text unverändert.
//!
//! **Was ersetzt wird** (in dieser Reihenfolge; ein Fund belegt seinen Bereich; Schwärzungen bleiben, aber nur in der
//! genauen Form `[REDACTED:<art>]` innerhalb einer Zeile):
//! 1. Text, der schon wie ein Platzhalter aussieht (`user-1000.slice`, `host-2` als echter Rechnername): Ist die Form noch
//!    nicht vergeben, bleibt sie stehen und wird für diese Sitzung nie vergeben (die Zählung überspringt sie). Ist sie
//!    schon vergeben (ein späterer Text enthält `ip-3`, das schon eine Adresse bedeutet), wird sie selbst pseudonymisiert
//!    und bekommt einen frischen Platzhalter. Damit bedeutet jeder vergebene Platzhalter im gesendeten Text genau einen
//!    Wert, die Rückübersetzung ist eindeutig, und Namen wie `user-1000.slice` bleiben lesbar (ein Präfix wie `@@ip-3`
//!    wäre auch im Rohtext möglich und für das Modell fremd). Platzhalter erkennt [`placeholder_at`]: Art, Bindestrich,
//!    Zahl ohne führende Null, davor kein Buchstabe (außer `\x2d`), dahinter keine Ziffer. Abschnitte einer Runde lernen
//!    vorher gemeinsam, so sind ihre Formen reserviert, bevor etwas vergeben wird. Nur allein stehende Formen belegen ihren
//!    Bereich sofort; eine Form in einem Wort (`ip-10-0-0-5.eu-central-1.compute.internal`) darf ein größerer Fund
//!    überdecken, sonst wird sie am Ende wie oben behandelt.
//! 2. Pfade unter `/home`: der Name des Ordners wird zu `user-N`. Der Rest wird **je Bestandteil** behandelt, weil ein
//!    Pfad zwei Arten von Namen mischt: solche, die ein Mensch vergibt (Ordner und Dateien mit Namen von Menschen, Kunden,
//!    Projekten), und solche, die eine Rolle oder ein Programm vorgibt und die für die Diagnose den Hinweis tragen
//!    (`pgdata` sagt „Datenordner von PostgreSQL“, `node_modules` „npm“, `.local/share/docker` „Docker“). Früher wurde der
//!    ganze Rest `path-N`; damit ging in Fall 4 des Messschritts genau dieser Hinweis verloren. Die Regel:
//!    - Ein Rest, der mit einem Punkt-Ordner beginnt (`.config`, `.local/share/xorg/Xorg.0.log`), sind Programmdaten und
//!      bleibt ganz. Ausgenommen die persönlichen Punkt-Pfade ([`PERSONAL_DOT_PATHS`]: Papierkorb, zuletzt benutzte
//!      Dateien, Vorschaubilder): darunter gilt die Regel für persönliche Ordner. Ordner, die die Config nennt
//!      (temp-logger), bleiben ganz.
//!    - Sonst bleibt ein Bestandteil, wenn er (oder sein Name ohne Endung) in [`TECHNICAL_NAMES`] steht (XDG-Ordner wie
//!      `Downloads`/`Dokumente`, `projects`, `src`, `docker`, `pgdata`, `postgres`, `node_modules`, `compose.yml`, …) oder
//!      nur aus Ziffern besteht. Eine feste Liste statt einer Schätzung („sieht technisch aus“), weil Projektnamen wie
//!      `dataroom` oder `kunde-x` genauso aussehen wie Programmnamen; was die Liste nicht kennt, gilt als persönlich.
//!    - Ab einem Punkt-Ordner tiefer im Pfad (`.git`, `.venv`) und unter [`PROGRAM_DIRS`] (`node_modules`,
//!      `site-packages`, `pgdata`, …) vergibt nur noch ein Programm Namen; der Rest bleibt.
//!    - Jeder andere Bestandteil wird ohne seine Endung `path-N` (`Angebot.odt` → `path-4.odt`, `Urlaub.tar.gz` →
//!      `path-8.tar.gz`); gleiche Bestandteile bekommen in der ganzen Sitzung denselben Platzhalter, auch in anderen
//!      Pfaden und Abschnitten (`Kunden` ist überall `path-2`), so bleibt erkennbar, dass zwei Pfade im selben Ordner liegen:
//!      `/home/blub/projects/dataroom/pgdata/base/1` → `/home/user-1/projects/path-1/pgdata/base/1`,
//!      `/home/blub/Dokumente/Kunden/Müller/Angebot.odt` → `/home/user-1/Dokumente/path-2/path-3/path-4.odt`.
//!
//!    In Kauf genommen: eine Endung verrät die Art der Datei, ein persönlicher Name, der zufällig ein Wort der Liste ist
//!    (ein Projekt namens `docker`), bleibt stehen, und Namen unter `.config` & Co. (z. B. ein Profilname) bleiben lesbar.
//!    Auch in `file:///home/…`, `//home/…` und JSON-escapt (`\/home\/…`). Als Name gelernt wird nur der eigene Ordner und
//!    der Ordner eines Kontos ([`Identity::accounts`]); andere Ordner (`/home/git`, `/home/data`) werden nur an dieser
//!    Stelle ersetzt, sonst würde jedes Wort `git` im Bericht ersetzt. Ein Home außerhalb von `/home` wird als Ganzes
//!    `path-N`.
//! 3. E-Mail-Adressen (`lokal@domain.tld`): bei bekannten Anbietern ([`MAIL_PROVIDERS`]) nur der lokale Teil
//!    (`email-1@gmail.com`), sonst die ganze Adresse (die Domain kann die Firma nennen). Bleiben: Rollen (`git@`,
//!    `noreply@`), Units (`getty@tty1.service`), `user@host:/pfad` (sshfs, git) und Benutzer in URLs.
//! 4. SSIDs hinter `SSID`/`ssid=`/`SSID:` (in Anführungszeichen, als Wort, hinter `:` bis zum Zeilenende), in
//!    `Config: added 'ssid' value '…'` und in den Meldungen des NetworkManager (`access point '…'`, `(wifi) connection
//!    '…'`) und von iwd (`Connected to "…"`); einmal erkannt, auch an jeder anderen Stelle.
//! 5. Rechnernamen mit lokaler Endung (`.ts.net` von Tailscale, `.local`, `.lan`, `.home`, `.home.arpa`, `.internal`,
//!    `.localdomain`, `.fritz.box`, `.intranet`) als Ganzes.
//! 6. MAC-Adressen (6 oder 8 Byte, dazu die 14 Byte des `MAC=`-Feldes von Netfilter und 20 Byte von InfiniBand). Bleiben:
//!    `00:00:00:00:00:00`, Broadcast und Gruppenadressen (Bit 0 im ersten Byte: `01:00:5e:…`, `33:33:…`), die keine
//!    Netzwerkkarte als Absender trägt.
//! 7. IPv6 und IPv4 (auch `::ffff:a.b.c.d` nach den Regeln für IPv4). Bleiben, weil sie niemanden kennzeichnen und für
//!    die Diagnose zählen: `0.0.0.0`, `::` (unspezifiziert), `127.0.0.0/8`, `::1` (Loopback), Broadcast
//!    `255.255.255.255`, Netzmasken (`255.255.255.0`), Multicast (`224.0.0.0/4`, `ff00::/8`, z. B. mDNS), die
//!    Dokumentationsnetze (`192.0.2.0/24`, `198.51.100.0/24`, `203.0.113.0/24`, `2001:db8::/32`), Tailscales MagicDNS
//!    (`100.100.100.100`, `fd7a:115c:a1e0::53`, bei jedem Tailnet gleich) und die bekannten öffentlichen Resolver
//!    ([`PUBLIC_RESOLVERS`]: Cloudflare, Google, Quad9, OpenDNS), weil „DNS zeigt auf 1.1.1.1“ ein Befund ist und kein
//!    persönliches Datum. **Alle anderen öffentlichen Adressen werden ersetzt**: in Logs ist das oft die eigene
//!    WAN-Adresse (Standort) oder ein Gegenüber; private (`10/8`, `172.16/12`, `192.168/16`), CGNAT/Tailscale
//!    (`100.64/10`) und Link-local (`169.254/16`, `fe80::/10`, deren Schnittstellen-ID oft die MAC enthält) ebenso.
//!    Dazu die Machine-ID ([`Identity::machine_id`], als 32 Hex-Zeichen und als UUID) als `id-N`.
//! 8. Benutzername und Rechnername der Sitzung ([`Identity`]), weitere Konten ([`Identity::accounts`]) und Namen, die die
//!    Proben strukturell zeigen: die Rechnerspalte der Journal-Zeilen (`short-iso`), Benutzer aus sshd-, PAM-, sudo- und
//!    audit-Zeilen (`for user X`, `for X from`, `user=X`, `X(uid=N)` mit N ≥ 1000; ohne Systemkonten), das Ziel von
//!    `net_reach` ([`Pseudonyms::learn`]), und die Namen, die `net_tailscale` und `net_mounts` strukturiert melden
//!    ([`Pseudonyms::learn_names`]: Knoten und Gegenstellen, auch mit Leerzeichen; Server, entfernte Benutzer, Freigaben
//!    als `//server/freigabe`). Ganze Wörter ohne Rücksicht auf Groß-/Kleinschreibung, Namen ab 4 Zeichen auch neben `-`,
//!    `_`, `.` und `\x2d` (`home-blub-nas.mount`, `blub--vg-root`, `blub_db_1`); allgemeine Wörter ([`GENERIC_NAMES`]:
//!    `localhost`, `server`, `nas`, …) bleiben, sie verraten nichts und kommen in Meldungen als Wörter vor. Ein Name, der Benutzer und Rechner ist (auf blub
//!    beides `blub`), bekommt einen Platzhalter (`user-1`): das stimmt, und das Modell sieht, dass beide gleich sind.
//!
//! Nicht erkannt: Namen von Menschen im Fließtext, Rechnernamen ohne lokale Endung, die keine Probe strukturell zeigt
//! (z. B. nur in einer Container-Meldung), Namen unter 4 Zeichen neben `-`/`_`/`.`. Gelernte Namen gelten ab
//! dem Lernen; Abschnitte einer Runde lernen vor dem Ersetzen gemeinsam ([`crate::diagnose::filter::filter_outputs`]),
//! schon gesendete Runden werden nicht nachträglich geändert.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::net::{Ipv4Addr, Ipv6Addr};

/// Art eines Platzhalters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Category {
    User,
    Host,
    Ip,
    Mac,
    Ssid,
    Path,
    Email,
    Id,
}

impl Category {
    pub const ALL: [Category; 8] = [Category::User, Category::Host, Category::Ip, Category::Mac, Category::Ssid, Category::Path, Category::Email, Category::Id];

    /// Präfix des Platzhalters und Name in der Vorschau (`user`, `host`, `ip`, `mac`, `ssid`, `path`, `email`, `id`).
    pub fn prefix(self) -> &'static str {
        match self {
            Category::User => "user",
            Category::Host => "host",
            Category::Ip => "ip",
            Category::Mac => "mac",
            Category::Ssid => "ssid",
            Category::Path => "path",
            Category::Email => "email",
            Category::Id => "id",
        }
    }

    /// Englische Bezeichnung für die Vorschau (wie `secrets::describe` für die Schwärzungen).
    pub fn describe(self) -> &'static str {
        match self {
            Category::User => "User name",
            Category::Host => "Host name",
            Category::Ip => "IP address",
            Category::Mac => "MAC address",
            Category::Ssid => "Wi-Fi name",
            Category::Path => "Personal path",
            Category::Email => "E-mail address",
            Category::Id => "Machine ID",
        }
    }

    fn index(self) -> usize {
        Category::ALL.iter().position(|c| *c == self).unwrap_or(0)
    }

    fn from_prefix(p: &str) -> Option<Category> {
        Category::ALL.into_iter().find(|c| c.prefix() == p)
    }
}

/// Wer die Sitzung führt: Benutzername, Home-Ordner und eigener Rechnername (im Daemon aus `getpwuid`/`gethostname`).
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Identity {
    pub user: String,
    pub home: String,
    pub hostname: String,
    /// Ordner unter dem Home, die die Config nennt und darum lesbar bleiben (z. B. `temperature-log`), relativ zum Home.
    pub keep_dirs: Vec<String>,
    /// Weitere lokale Konten von Menschen (`/etc/passwd`, uid ≥ 1000 außer `nobody`; im Daemon gelesen): werden wie der
    /// Benutzer ersetzt, auch als Ordner unter `/home`.
    pub accounts: Vec<String>,
    /// Inhalt von `/etc/machine-id` (32 Hex-Zeichen; leer = unbekannt): eine dauerhafte Kennung des Rechners, die systemd
    /// vertraulich zu behandeln verlangt (machine-id(5)). Wird `id-N`, auch in UUID-Schreibweise.
    pub machine_id: String,
}

/// Allgemeine Rechnernamen: verraten nichts und kommen als Wörter in Meldungen vor, bleiben also stehen.
pub const GENERIC_NAMES: &[&str] = &[
    "localhost",
    "localhost.localdomain",
    "root",
    "kernel",
    "systemd",
    "docker",
    "linux",
    "arch",
    "archlinux",
    "ubuntu",
    "debian",
    "fedora",
    "android",
    "windows",
    "macos",
    "iphone",
    "ipad",
    "server",
    "client",
    "host",
    "desktop",
    "laptop",
    "phone",
    "router",
    "gateway",
    "nas",
    "pc",
    "pi",
    "raspberrypi",
    "unknown",
    "none",
    "null",
    "default",
    "self",
    "peer",
];

/// Namen hinter `user=`, `for user` usw., die keinen Menschen bezeichnen: Füllwörter und übliche Systemkonten.
const NOT_A_USER: &[&str] = &[
    "unknown",
    "invalid",
    "nobody",
    "user",
    "users",
    "from",
    "the",
    "bin",
    "daemon",
    "mail",
    "ftp",
    "http",
    "git",
    "gdm",
    "sddm",
    "lightdm",
    "polkitd",
    "dbus",
    "colord",
    "rtkit",
    "avahi",
    "nginx",
    "www-data",
    "postgres",
    "mysql",
    "redis",
    "sshd",
    "uuidd",
    "dnsmasq",
    "ntp",
    "chrony",
    "geoclue",
    "cups",
    "saned",
    "flatpak",
    "alpm",
    "tss",
    "usbmux",
    "nm-openconnect",
    "nm-openvpn",
    "brltty",
    "named",
    "rpc",
];

/// Lokale Endungen von Rechnernamen (ohne führenden Punkt, klein).
const LOCAL_SUFFIXES: &[&str] = &["ts.net", "local", "lan", "home", "home.arpa", "internal", "localdomain", "fritz.box", "intranet"];

/// Öffentliche Resolver, die stehen bleiben (Anycast-Dienste, für alle gleich).
pub const PUBLIC_RESOLVERS: &[&str] = &[
    "1.1.1.1",
    "1.0.0.1",
    "8.8.8.8",
    "8.8.4.4",
    "9.9.9.9",
    "149.112.112.112",
    "208.67.222.222",
    "208.67.220.220",
    "2606:4700:4700::1111",
    "2606:4700:4700::1001",
    "2001:4860:4860::8888",
    "2001:4860:4860::8844",
    "2620:fe::fe",
    "2620:fe::9",
];

/// Teile unter dem Home, die trotz Punkt-Ordner persönlich sind (Dateinamen gelöschter Dokumente, zuletzt benutzte Dateien).
const PERSONAL_DOT_PATHS: &[&str] = &[".local/share/Trash", ".local/share/recently-used.xbel", ".cache/thumbnails"];

/// Bestandteile von Pfaden unter dem Home, die eine Rolle oder eine Software nennen und keinen Menschen, keine Firma und
/// kein Projekt (Vergleich ohne Groß/klein, mit dem ganzen Bestandteil oder dem Namen ohne Endung; Regel im Modulkopf):
/// die Ordner der XDG-Benutzerverzeichnisse auf Englisch und Deutsch, übliche Ordner von Entwicklung und Betrieb,
/// Datenordner bekannter Software, Größen der Vorschaubilder, Dateinamen, die ein Programm vorgibt.
pub const TECHNICAL_NAMES: &[&str] = &[
    // XDG-Benutzerverzeichnisse (xdg-user-dirs, en und de)
    "desktop",
    "documents",
    "downloads",
    "music",
    "pictures",
    "public",
    "templates",
    "videos",
    "schreibtisch",
    "dokumente",
    "musik",
    "bilder",
    "öffentlich",
    "vorlagen",
    "screenshots",
    "bildschirmfotos",
    // Entwicklung und Betrieb
    "projects",
    "projekte",
    "src",
    "source",
    "sources",
    "code",
    "dev",
    "git",
    "repos",
    "repositories",
    "workspace",
    "bin",
    "sbin",
    "lib",
    "lib64",
    "libexec",
    "include",
    "share",
    "build",
    "dist",
    "out",
    "target",
    "release",
    "debug",
    "tmp",
    "temp",
    "cache",
    "logs",
    "log",
    "data",
    "_data",
    "db",
    "database",
    "backup",
    "backups",
    "config",
    "conf",
    "etc",
    "var",
    "run",
    "scripts",
    "test",
    "tests",
    "docs",
    "doc",
    "assets",
    "static",
    "vendor",
    "packages",
    "pkg",
    "venv",
    "env",
    "files",
    "info",
    "models",
    "go",
    "cargo",
    "npm",
    "node",
    "python",
    "rust",
    "applications",
    "fonts",
    "icons",
    "themes",
    // Software und ihre Daten
    "docker",
    "compose",
    "volumes",
    "containers",
    "podman",
    "pgdata",
    "postgres",
    "postgresql",
    "mysql",
    "mariadb",
    "redis",
    "mongodb",
    "sqlite",
    "nginx",
    "caddy",
    "www",
    "html",
    "ollama",
    "flatpak",
    "snap",
    "appimages",
    "steam",
    "steamapps",
    "common",
    "compatdata",
    "wine",
    "node_modules",
    "site-packages",
    "dist-packages",
    "__pycache__",
    // Größen der Vorschaubilder (freedesktop Thumbnail Spec)
    "normal",
    "large",
    "x-large",
    "xx-large",
    "fail",
    // Dateinamen, die ein Programm vorgibt
    "readme",
    "makefile",
    "dockerfile",
    "docker-compose",
    "package",
    "package-lock",
    "requirements",
    "pyproject",
    "setup",
    "settings",
    "index",
    "main",
    "default",
    "output",
    "nohup",
    "core",
];

/// Ordner, unter denen nur noch ein Programm Namen vergibt (Pakete, Cache, Datenbankdateien): der Rest des Pfades bleibt.
const PROGRAM_DIRS: &[&str] = &["node_modules", "site-packages", "dist-packages", "__pycache__", "pgdata"];

/// Journal-Proben mit Zeilen im Format `short-iso` (Zeit, Rechner, Programm).
const JOURNAL_PROBES: &[&str] = &["boot_end", "kernel_errors", "unit_journal", "oom"];

/// Zuordnung einer Sitzung. Nie loggen, nie senden.
pub struct Pseudonyms {
    enabled: bool,
    identity: Identity,
    /// (Art, normierter Wert) → Platzhalter
    forward: HashMap<(Category, String), String>,
    /// Platzhalter → Wert, wie er zuerst vorkam
    back: HashMap<String, String>,
    next: [usize; Category::ALL.len()],
    /// Namen für den Wortvergleich (klein) mit ihrer Art: Benutzer und Rechner
    names: BTreeMap<String, Category>,
    /// erkannte SSIDs (genau)
    ssids: Vec<String>,
    /// Platzhalter-Formen, die im Rohtext standen, bevor sie vergeben waren: werden nie vergeben
    reserved: HashSet<String>,
}

impl std::fmt::Debug for Pseudonyms {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // nie Werte oder Zuordnung, nur Zahlen
        f.debug_struct("Pseudonyms").field("enabled", &self.enabled).field("placeholders", &self.back.len()).finish()
    }
}

/// Zählung je Art (Vorkommen im Text), in der Reihenfolge von [`Category::ALL`], ohne Nullen.
pub type Counts = Vec<(Category, usize)>;

impl Pseudonyms {
    pub fn new(identity: Identity, enabled: bool) -> Pseudonyms {
        let mut p = Pseudonyms {
            enabled,
            identity,
            forward: HashMap::new(),
            back: HashMap::new(),
            next: [0; Category::ALL.len()],
            names: BTreeMap::new(),
            ssids: Vec::new(),
            reserved: HashSet::new(),
        };
        let user = p.identity.user.clone();
        let host = p.identity.hostname.clone();
        if usable_name(&user) {
            p.names.insert(user.to_ascii_lowercase(), Category::User);
            p.placeholder(Category::User, &user.to_ascii_lowercase(), &user);
        }
        if usable_name(&host) && !p.names.contains_key(&host.to_ascii_lowercase()) {
            p.names.insert(host.to_ascii_lowercase(), Category::Host);
            p.placeholder(Category::Host, &host.to_ascii_lowercase(), &host);
        }
        for account in p.identity.accounts.clone() {
            if usable_name(&account) {
                p.names.entry(account.to_ascii_lowercase()).or_insert(Category::User);
            }
        }
        p
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    /// Platzhalter für einen Wert (neu oder schon vergeben).
    fn placeholder(&mut self, cat: Category, key: &str, original: &str) -> String {
        if let Some(p) = self.forward.get(&(cat, key.to_string())) {
            return p.clone();
        }
        let i = cat.index();
        let p = loop {
            self.next[i] += 1;
            let p = format!("{}-{}", cat.prefix(), self.next[i]);
            if !self.reserved.contains(&p) {
                break p;
            }
        };
        self.forward.insert((cat, key.to_string()), p.clone());
        self.back.insert(p.clone(), original.to_string());
        p
    }

    fn learn_host(&mut self, name: &str) {
        let name = name.trim_end_matches('.');
        if !usable_name(name) || !name.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'.' || c == b'_') || ip_like(name) {
            return;
        }
        self.names.entry(name.to_ascii_lowercase()).or_insert(Category::Host);
    }

    /// Lernt Namen, die eine Probe strukturiert meldet ([`crate::diagnose::probes::ProbeOutput::names`]: Rechner und
    /// Gegenstellen von `net_tailscale`, Server, entfernte Benutzer und Freigaben von `net_mounts`). Namen dürfen
    /// Leerzeichen und Apostrophe enthalten (`Daniel's iPad`) und werden als Ganzes ersetzt.
    pub fn learn_names(&mut self, names: &[(Category, String)]) {
        if !self.enabled {
            return;
        }
        for (cat, name) in names {
            let name = name.trim().trim_end_matches('.');
            let ok = usable_name(name) && name.len() <= 128 && !name.chars().any(char::is_control) && !ip_like(name) && name.is_ascii();
            if ok && placeholder_at(name.as_bytes(), 0).is_none_or(|(_, e)| e != name.len()) {
                self.names.entry(name.to_ascii_lowercase()).or_insert(*cat);
            }
        }
    }

    /// Lernt Benutzernamen aus der Struktur von Log-Zeilen (sshd, PAM, sudo, audit): `for user X`, `for X from`,
    /// `Invalid user X from`, `user=X`, `USER=X`, `ruser=X`, `acct="X"`, `X(uid=N)`.
    fn learn_users(&mut self, text: &str) {
        let b = text.as_bytes();
        let mut found: Vec<&str> = Vec::new();
        let word_at = |i: usize| -> &str {
            let i = i + usize::from(b.get(i) == Some(&b'"'));
            let e = i + b[i.min(b.len())..]
                .iter()
                .position(|&c| !(c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-' | b'.' | b'$')))
                .unwrap_or(b.len() - i.min(b.len()));
            text.get(i..e).unwrap_or("").trim_end_matches('.')
        };
        for marker in ["for user ", "for invalid user ", "Invalid user ", "invalid user "] {
            let mut from = 0;
            while let Some(p) = find(b, marker.as_bytes(), from) {
                from = p + 1;
                found.push(word_at(p + marker.len()));
            }
        }
        // `Accepted publickey for anna from`, `Failed password for anna from`
        let mut from = 0;
        while let Some(p) = find(b, b" for ", from) {
            from = p + 1;
            let w = word_at(p + 5);
            if text.get(p + 5 + w.len()..).is_some_and(|r| r.starts_with(" from ")) {
                found.push(w);
            }
        }
        for key in ["user=", "USER=", "ruser=", "acct=", "logname="] {
            let mut from = 0;
            while let Some(p) = find(b, key.as_bytes(), from) {
                from = p + 1;
                if p == 0 || !b[p - 1].is_ascii_alphanumeric() && b[p - 1] != b'_' {
                    found.push(word_at(p + key.len()));
                }
            }
        }
        let mut from = 0;
        while let Some(p) = find(b, b"(uid=", from) {
            from = p + 1;
            let s = b[..p].iter().rposition(|&c| !(c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-' | b'.'))).map_or(0, |x| x + 1);
            found.push(&text[s..p]);
        }
        for name in found {
            // `gdm(uid=120)`: ein Systemkonto (uid unter 1000), kein Mensch
            let off = name.as_ptr() as usize - text.as_ptr() as usize;
            let uid: Option<u32> = text[off + name.len()..].strip_prefix("(uid=").and_then(|r| r[..r.find(')').unwrap_or(r.len())].parse().ok());
            if uid.is_some_and(|u| u < 1000) || name.starts_with("systemd-") {
                continue;
            }
            let first_ok = name.bytes().next().is_some_and(|c| c.is_ascii_alphabetic() || c == b'_');
            if first_ok
                && usable_name(name)
                && name.len() <= 32
                && placeholder_at(name.as_bytes(), 0).is_none()
                && !NOT_A_USER.contains(&name.to_ascii_lowercase().as_str())
            {
                self.names.entry(name.to_ascii_lowercase()).or_insert(Category::User);
            }
        }
    }

    /// Lernt Namen, die die Probe strukturell zeigt (siehe Modulkopf, Punkt 7), und SSIDs. Ändert keinen Text.
    pub fn learn(&mut self, probe: &str, text: &str) {
        if !self.enabled {
            return;
        }
        self.reserve_literals(text);
        for (_, _, v) in ssid_spans(text) {
            if !self.ssids.contains(&v) {
                self.ssids.push(v);
            }
        }
        self.learn_users(text);
        if JOURNAL_PROBES.contains(&probe) {
            for line in text.lines() {
                let mut it = line.split_whitespace();
                if let (Some(ts), Some(host)) = (it.next(), it.next()) {
                    if iso_timestamp(ts) {
                        self.learn_host(host);
                    }
                }
            }
        }
        if probe == "net_reach" {
            for line in text.lines() {
                if let Some(rest) = line.strip_prefix("PING ") {
                    self.learn_host(rest.split_whitespace().next().unwrap_or(""));
                }
            }
        }
    }

    /// Ersetzt persönliche Daten durch Platzhalter; liefert den Text und die Zahl der Ersetzungen je Art.
    pub fn apply(&mut self, text: &str) -> (String, Counts) {
        if !self.enabled || text.is_empty() {
            return (text.to_string(), Vec::new());
        }
        let mut c = Collect { b: text.as_bytes(), claimed: vec![false; text.len()], spans: Vec::new() };
        // Schwärzungen bleiben, wie sie sind: nur die genaue Form `[REDACTED:<art>]` in einer Zeile (ein `[REDACTED:` ohne
        // `]`, etwa aus einem Container-Log, schützt nichts dahinter)
        let mut from = 0;
        while let Some(p) = find(c.b, b"[REDACTED:", from) {
            let kind = c.b[p + 10..].iter().take_while(|x| x.is_ascii_lowercase() || **x == b'_').count();
            let end = p + 10 + kind;
            if kind > 0 && c.b.get(end) == Some(&b']') {
                c.claim(p, end + 1);
            }
            from = end.max(p + 1);
        }
        // 1. schon wie ein Platzhalter: vergebene werden ersetzt, alle anderen bleiben und werden nie vergeben. Nur
        //    alleinstehende Formen belegen ihren Bereich sofort; eine Form mitten in einem größeren Fund
        //    (`ip-10-0-0-5.eu-central-1.compute.internal`, `user-2.fritz.box`) darf von diesem überdeckt werden (Schritt 9).
        self.reserve_literals(text);
        let mut inner_literals = Vec::new();
        let mut i = 0;
        while i < c.b.len() {
            if let Some((cat, end)) = placeholder_at(c.b, i) {
                let alone = (i == 0 || !joins(c.b[i - 1])) && c.b.get(end).is_none_or(|&x| !joins(x));
                if !alone {
                    inner_literals.push((i, end, cat));
                } else if self.back.contains_key(&text[i..end]) {
                    c.push(i, end, Repl::Literal(cat));
                } else {
                    c.claim(i, end);
                }
                i = end;
            } else {
                i += 1;
            }
        }
        // 2. Pfade unter /home und das Home
        self.home_paths(&mut c, text);
        // 3. E-Mail-Adressen
        email_spans(&mut c);
        // 4. SSIDs (erkannte und gelernte)
        for (s, e, v) in ssid_spans(text) {
            if !self.ssids.contains(&v) {
                self.ssids.push(v);
            }
            c.push(s, e, Repl::Ssid);
        }
        for ssid in self.ssids.clone().into_iter().filter(|s| s.chars().count() >= 2) {
            let mut from = 0;
            while let Some(p) = find(c.b, ssid.as_bytes(), from) {
                let e = p + ssid.len();
                if !c.b.get(p.wrapping_sub(1)).is_some_and(|x| p > 0 && x.is_ascii_alphanumeric()) && !c.b.get(e).is_some_and(u8::is_ascii_alphanumeric) {
                    c.push(p, e, Repl::Ssid);
                }
                from = p + 1;
            }
        }
        // 5. Rechnernamen mit lokaler Endung
        host_suffix_spans(&mut c);
        // 6. MAC, 7. IP, Machine-ID
        mac_spans(&mut c);
        ip_spans(&mut c);
        self.machine_id_spans(&mut c);
        // 8. Namen als ganze Wörter; Namen ab 4 Zeichen auch neben `-`, `_`, `.` und `\x2d` (`home-blub-nas.mount`,
        //    `blub--vg-root`, `blub_db_1`, `dev-mapper-blub\x2dvg`), kürzere nur allein
        let lower = text.to_ascii_lowercase();
        let lb = lower.as_bytes();
        // längere Namen zuerst (`//nas/Daniel` vor `nas`, `blub-laptop` vor `blub`)
        let mut names: Vec<(String, Category)> = self.names.clone().into_iter().collect();
        names.sort_by_key(|(n, _)| std::cmp::Reverse(n.len()));
        for (name, cat) in names {
            let relaxed = name.len() >= 4;
            let mut from = 0;
            while let Some(p) = find(lb, name.as_bytes(), from) {
                let e = p + name.len();
                let before_ok = p == 0
                    || if relaxed {
                        !lb[p - 1].is_ascii_alphanumeric() || (p >= 4 && &lb[p - 4..p] == b"\\x2d")
                    } else {
                        !is_word(lb[p - 1]) && lb[p - 1] != b'.' && lb[p - 1] != b'-'
                    };
                let after_ok = e >= lb.len() || if relaxed { !lb[e].is_ascii_alphanumeric() } else { !is_word(lb[e]) && lb[e] != b'-' };
                if before_ok && after_ok {
                    c.push(p, e, Repl::Name(cat));
                }
                from = p + 1;
            }
        }
        // 9. Platzhalter-Formen in einem Wort, die kein größerer Fund überdeckt hat: vergebene werden ersetzt
        for (s, e, cat) in inner_literals {
            if self.back.contains_key(&text[s..e]) {
                c.push(s, e, Repl::Literal(cat));
            }
        }

        c.spans.sort_by_key(|s| s.0);
        let mut out = String::with_capacity(text.len());
        let mut counts = [0usize; Category::ALL.len()];
        let mut at = 0;
        for (s, e, repl) in c.spans {
            let original = &text[s..e];
            let (cat, key) = match repl {
                Repl::Literal(cat) => (cat, format!("literal:{original}")),
                Repl::Name(cat) => (cat, original.to_ascii_lowercase()),
                Repl::Path => (Category::Path, original.replace("\\/", "/")),
                Repl::PathPart => (Category::Path, format!("part:{original}")),
                Repl::Ssid => (Category::Ssid, original.to_string()),
                Repl::Mac => (Category::Mac, original.to_ascii_lowercase()),
                Repl::Ip(ref norm) => (Category::Ip, norm.clone()),
                Repl::Email(ref address) => (Category::Email, address.clone()),
                Repl::MachineId => (Category::Id, "machine-id".to_string()),
            };
            let p = self.placeholder(cat, &key, original);
            out.push_str(&text[at..s]);
            out.push_str(&p);
            counts[cat.index()] += 1;
            at = e;
        }
        out.push_str(&text[at..]);
        (out, Category::ALL.into_iter().zip(counts).filter(|(_, n)| *n > 0).collect())
    }

    /// Die Machine-ID der Sitzung als 32 Hex-Zeichen oder in UUID-Schreibweise, ohne Rücksicht auf Groß/klein.
    fn machine_id_spans(&self, c: &mut Collect) {
        let id = self.identity.machine_id.trim().to_ascii_lowercase();
        if id.len() != 32 || !id.bytes().all(|x| x.is_ascii_hexdigit()) {
            return;
        }
        let uuid = format!("{}-{}-{}-{}-{}", &id[..8], &id[8..12], &id[12..16], &id[16..20], &id[20..]);
        let lower = c.b.to_ascii_lowercase();
        for form in [id.as_bytes(), uuid.as_bytes()] {
            let mut from = 0;
            while let Some(p) = find(&lower, form, from) {
                let e = p + form.len();
                if (p == 0 || !lower[p - 1].is_ascii_alphanumeric()) && lower.get(e).is_none_or(|x| !x.is_ascii_alphanumeric()) {
                    c.push(p, e, Repl::MachineId);
                }
                from = p + 1;
            }
        }
    }

    /// Merkt Platzhalter-Formen des Rohtextes, die noch nicht vergeben sind (`user-1000.slice`, `host-2`), damit sie nie
    /// vergeben werden.
    fn reserve_literals(&mut self, text: &str) {
        let b = text.as_bytes();
        let mut i = 0;
        while i < b.len() {
            if let Some((_, end)) = placeholder_at(b, i) {
                let t = &text[i..end];
                if !self.back.contains_key(t) {
                    self.reserved.insert(t.to_string());
                }
                i = end;
            } else {
                i += 1;
            }
        }
    }

    /// Übersetzt Platzhalter (wie [`placeholder_at`] sie erkennt) in die echten Werte zurück; unbekannte bleiben.
    /// Nur für die Anzeige im Panel, nie für Text, der gesendet oder geloggt wird.
    pub fn restore(&self, text: &str) -> String {
        if !self.enabled {
            return text.to_string();
        }
        let b = text.as_bytes();
        let mut out = String::with_capacity(text.len());
        let (mut i, mut at) = (0, 0);
        while i < b.len() {
            if let Some((_, end)) = placeholder_at(b, i) {
                if let Some(v) = self.back.get(&text[i..end]) {
                    out.push_str(&text[at..i]);
                    out.push_str(v);
                    at = end;
                }
                i = end;
            } else {
                i += 1;
            }
        }
        out.push_str(&text[at..]);
        out
    }

    /// Die Platzhalter dieser Sitzung in `text` (dieselbe Erkennung wie [`Pseudonyms::restore`]): Anfang, Ende (Bytes)
    /// und der echte Wert. Unbekannte Formen fehlen; ohne Pseudonymisieren ist die Liste leer. Nur für die Anzeige im
    /// Panel ([`super::restore`]), nie für Text, der gesendet oder geloggt wird.
    pub fn known_placeholders(&self, text: &str) -> Vec<(usize, usize, String)> {
        let mut out = Vec::new();
        if !self.enabled {
            return out;
        }
        let b = text.as_bytes();
        let mut i = 0;
        while i < b.len() {
            if let Some((_, end)) = placeholder_at(b, i) {
                if let Some(v) = self.back.get(&text[i..end]) {
                    out.push((i, end, v.clone()));
                }
                i = end;
            } else {
                i += 1;
            }
        }
        out
    }

    fn home_paths(&mut self, c: &mut Collect, text: &str) {
        // `/home/…`, auch in `file:///home/…` und `//home/…`, und JSON-escapt `\/home\/…`
        self.home_paths_with(c, text, b"/");
        self.home_paths_with(c, text, b"\\/");
        // Home außerhalb von /home: als Ganzes
        let b = c.b;
        let home = self.identity.home.trim_end_matches('/').to_string();
        if home.len() > 1 && !home.starts_with("/home/") {
            let mut from = 0;
            while let Some(p) = find(b, home.as_bytes(), from) {
                let e = p + home.len();
                let before_ok = p == 0 || !(is_word(b[p - 1]) || matches!(b[p - 1], b'.' | b'-' | b'/'));
                let after_ok = e >= b.len() || !(is_word(b[e]) || b[e] == b'-' || b[e] == b'.');
                if before_ok && after_ok {
                    c.push(p, e, Repl::Path);
                }
                from = p + 1;
            }
        }
    }

    /// Pfade `<sep>home<sep><ordner><sep><rest>` mit dem Trenner `sep` (`/` oder JSON-escapt `\/`).
    fn home_paths_with(&mut self, c: &mut Collect, text: &str, sep: &[u8]) {
        let b = c.b;
        let pattern = [sep, b"home", sep].concat();
        let mut from = 0;
        while let Some(p) = find(b, &pattern, from) {
            from = p + 1;
            // davor kein Wort und kein Pfad; `//home/` und `file:///home/` zählen (mehrere `/` hinter einem Nicht-Wort)
            let before = |i: usize| is_word(b[i]) || matches!(b[i], b'.' | b'-' | b'~');
            if p > 0 && (before(p - 1) || (b[p - 1] == b'/' && p >= 2 && (before(p - 2) || (b[p - 2] == b'/' && p >= 3 && before(p - 3) && b[p - 3] != b':'))))
            {
                continue;
            }
            if sep == b"/" && p > 0 && b[p - 1] == b'\\' {
                continue;
            }
            let mut end = p + path_run(&b[p..]);
            while end > p && matches!(b[end - 1], b'.' | b':') {
                end -= 1;
            }
            let us = p + pattern.len();
            if us > end {
                continue;
            }
            let ue = find(&b[..end], sep, us).unwrap_or(end);
            if ue > us {
                let seg = &text[us..ue];
                let lower = seg.to_ascii_lowercase();
                // als Name lernen nur den eigenen Ordner und Ordner echter Konten; andere Ordner (`/home/git`, `/home/data`)
                // nur an dieser Stelle ersetzen, sonst würde jedes Wort `git` oder `data` im Bericht ersetzt
                let own = self.identity.home.trim_end_matches('/').strip_prefix("/home/").is_some_and(|h| h.eq_ignore_ascii_case(seg));
                let account = own || self.identity.accounts.iter().any(|a| a.eq_ignore_ascii_case(seg)) || self.names.contains_key(&lower);
                let cat = if account && usable_name(seg) { *self.names.entry(lower).or_insert(Category::User) } else { Category::User };
                c.push(us, ue, Repl::Name(cat));
            }
            let rest_start = ue + sep.len();
            if ue < end && rest_start < end {
                self.path_parts(c, text, rest_start, end, sep);
            }
            from = end.max(p + 1);
        }
    }

    /// Die Bestandteile von `text[start..end]` (Pfad unter `/home/<ordner>/`, Trenner `sep`) nach der Regel im Modulkopf:
    /// persönliche einzeln ersetzen (ohne Endung), technische stehen lassen.
    fn path_parts(&self, c: &mut Collect, text: &str, start: usize, end: usize, sep: &[u8]) {
        let b = c.b;
        let mut parts = Vec::new();
        let mut s = start;
        while s <= end {
            let e = find(&b[..end], sep, s).unwrap_or(end);
            parts.push((s, e));
            s = e + sep.len();
        }
        let names: Vec<&str> = parts.iter().map(|&(s, e)| &text[s..e]).collect();
        let starts_with = |prefix: &str| {
            let want: Vec<&str> = prefix.split('/').filter(|x| !x.is_empty()).collect();
            (!want.is_empty() && names.len() >= want.len() && names[..want.len()] == want[..]).then_some(want.len())
        };
        // Ordner aus der Config: ganz lesbar
        if self.identity.keep_dirs.iter().any(|d| starts_with(d).is_some()) {
            return;
        }
        // Punkt-Ordner sind Programmdaten und bleiben ganz, außer den persönlichen (Papierkorb, Vorschaubilder): dort
        // gilt unter dem Punkt-Ordner die Regel für persönliche Ordner, auch für Punkt-Namen
        let personal = PERSONAL_DOT_PATHS.iter().find_map(|p| starts_with(p));
        if personal.is_none() && names.first().is_some_and(|n| n.starts_with('.')) {
            return;
        }
        for (k, &(s, e)) in parts.iter().enumerate().skip(personal.unwrap_or(0)) {
            let name = names[k];
            if name.is_empty() || name == "." || name == ".." {
                continue;
            }
            let lower = name.to_lowercase();
            // ab hier schreibt ein Programm die Namen (`.git`, `node_modules`, `pgdata`)
            if (personal.is_none() && name.starts_with('.')) || PROGRAM_DIRS.contains(&lower.as_str()) {
                return;
            }
            let stem = &name[..stem_len(name)];
            let technical = |n: &str| TECHNICAL_NAMES.contains(&n.to_lowercase().as_str());
            if stem.is_empty() || technical(name) || technical(stem) || stem.bytes().all(|x| x.is_ascii_digit()) {
                continue;
            }
            c.push(s, s + stem.len(), Repl::PathPart);
            debug_assert!(s + stem.len() <= e);
        }
    }
}

/// Länge des Namens ohne Endung: höchstens zwei Endungen aus 1 bis 5 ASCII-Buchstaben und -Ziffern mit mindestens einem
/// Buchstaben (`Angebot.odt`, `Urlaub.tar.gz`); ein Punkt am Anfang ist keine Endung.
fn stem_len(name: &str) -> usize {
    let mut stem = name;
    for _ in 0..2 {
        match stem.rfind('.') {
            Some(d) if d > 0 => {
                let ext = &stem[d + 1..];
                if (1..=5).contains(&ext.len()) && ext.bytes().all(|x| x.is_ascii_alphanumeric()) && ext.bytes().any(|x| x.is_ascii_alphabetic()) {
                    stem = &stem[..d];
                } else {
                    break;
                }
            }
            _ => break,
        }
    }
    stem.len()
}

enum Repl {
    Literal(Category),
    Name(Category),
    /// ganzer Pfad (Home außerhalb von `/home`)
    Path,
    /// ein persönlicher Bestandteil eines Pfades unter `/home/<ordner>/` (ohne Endung)
    PathPart,
    Ssid,
    Mac,
    Ip(String),
    /// normierte Adresse (klein)
    Email(String),
    MachineId,
}

struct Collect<'a> {
    b: &'a [u8],
    claimed: Vec<bool>,
    spans: Vec<(usize, usize, Repl)>,
}

impl Collect<'_> {
    fn free(&self, s: usize, e: usize) -> bool {
        s < e && e <= self.b.len() && !self.claimed[s..e].iter().any(|&x| x)
    }

    fn claim(&mut self, s: usize, e: usize) {
        let e = e.min(self.claimed.len());
        for x in &mut self.claimed[s.min(e)..e] {
            *x = true;
        }
    }

    /// Nimmt einen Bereich, wenn er frei ist und auf Zeichengrenzen liegt.
    fn push(&mut self, s: usize, e: usize, r: Repl) {
        let boundary = |i: usize| i == self.b.len() || i < self.b.len() && (self.b[i] & 0xC0) != 0x80;
        if self.free(s, e) && boundary(s) && boundary(e) {
            self.claim(s, e);
            self.spans.push((s, e, r));
        }
    }
}

fn is_word(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

fn usable_name(n: &str) -> bool {
    n.len() >= 2 && !GENERIC_NAMES.contains(&n.to_ascii_lowercase().as_str())
}

/// Platzhalter ab `i`: Art, `-`, Zahl ohne führende Null; davor kein Buchstabe und kein `_`, dahinter keine Ziffer und
/// kein `_`. Liefert die Art und das Ende. Dieselbe Regel für das Erkennen im Rohtext und für die Rückübersetzung. Eine
/// Ziffer davor und ein Buchstabe dahinter sind erlaubt, damit zwei unmittelbar aufeinander folgende Platzhalter
/// (`ip-1user-2`) beide erkannt werden; kein Erkenner ersetzt etwas direkt hinter einem Buchstaben oder vor einer Ziffer.
pub fn placeholder_at(b: &[u8], i: usize) -> Option<(Category, usize)> {
    // `\x2d` (escaptes `-` in Unit-Namen) davor zählt als Grenze
    if i > 0 && b[i - 1].is_ascii_alphabetic() && !(i >= 4 && &b[i - 4..i] == b"\\x2d") {
        return None;
    }
    let letters = b[i..].iter().take_while(|c| c.is_ascii_lowercase()).count();
    let prefix = std::str::from_utf8(&b[i..i + letters]).ok()?;
    let cat = Category::from_prefix(prefix)?;
    let dash = i + letters;
    if b.get(dash) != Some(&b'-') {
        return None;
    }
    let digits = b[dash + 1..].iter().take_while(|c| c.is_ascii_digit()).count();
    let end = dash + 1 + digits;
    if digits == 0 || b[dash + 1] == b'0' {
        return None;
    }
    Some((cat, end))
}

/// Verbindet ein Zeichen neben einem Platzhalter ihn mit einem größeren Wort (`ip-10-0-0-5.…`, `user-1000.slice`)?
fn joins(c: u8) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, b'-' | b'.' | b'_')
}

/// Anbieter, deren Domain in einer E-Mail-Adresse lesbar bleibt (nur der lokale Teil wird `email-N`): sie verraten
/// niemanden, und „Gmail lehnt ab“ kann ein Befund sein.
const MAIL_PROVIDERS: &[&str] = &[
    "gmail.com",
    "googlemail.com",
    "outlook.com",
    "hotmail.com",
    "live.com",
    "msn.com",
    "icloud.com",
    "me.com",
    "mac.com",
    "yahoo.com",
    "gmx.at",
    "gmx.de",
    "gmx.net",
    "web.de",
    "posteo.de",
    "proton.me",
    "protonmail.com",
    "aon.at",
    "a1.net",
    "mailbox.org",
];

/// Lokale Teile, die eine Rolle statt eines Menschen bezeichnen (`git@github.com`, `noreply@…`): bleiben.
const ROLE_LOCALS: &[&str] = &["git", "root", "noreply", "no-reply", "donotreply", "postmaster", "mailer-daemon", "nobody", "abuse", "hostmaster", "webmaster"];

/// Endungen, die kein Domain-Ende sind (`getty@tty1.service`, `user@1000.service`, `pkg@1.2.3`).
const NOT_A_TLD: &[&str] =
    &["service", "socket", "target", "mount", "automount", "timer", "scope", "slice", "path", "device", "swap", "conf", "txt", "log", "json", "sh"];

/// E-Mail-Adressen `lokal@domain.tld`: der lokale Teil aus `A-Za-z0-9._%+-`, die Domain mit Punkt und einer Endung aus
/// Buchstaben (nicht von Units, Dateien). Bei bekannten Anbietern wird nur der lokale Teil ersetzt, sonst die ganze
/// Adresse (die Domain kann die Firma nennen). Benutzer in URLs (`https://user@host`) sind keine Adresse.
fn email_spans(c: &mut Collect) {
    let b = c.b;
    let local_char = |x: u8| x.is_ascii_alphanumeric() || matches!(x, b'.' | b'_' | b'%' | b'+' | b'-');
    let mut from = 0;
    while let Some(at) = find(b, b"@", from) {
        from = at + 1;
        let ls = b[..at].iter().rposition(|&x| !local_char(x)).map_or(0, |x| x + 1);
        let de = at + 1 + b[at + 1..].iter().position(|&x| !(x.is_ascii_alphanumeric() || x == b'.' || x == b'-')).unwrap_or(b.len() - at - 1);
        let mut de = de;
        while de > at + 1 && matches!(b[de - 1], b'.' | b'-') {
            de -= 1;
        }
        // `user@host:/pfad` (sshfs, scp, git) und `https://user@host` sind Benutzer an einem Rechner, keine Adresse
        if ls == at || b[ls] == b'.' || b[at - 1] == b'.' || (ls >= 2 && &b[ls - 2..ls] == b"//") || b.get(de) == Some(&b':') {
            continue;
        }
        let (Ok(local), Ok(domain)) = (std::str::from_utf8(&b[ls..at]), std::str::from_utf8(&b[at + 1..de])) else { continue };
        let domain_l = domain.to_ascii_lowercase();
        let labels: Vec<&str> = domain_l.split('.').collect();
        let tld = labels.last().copied().unwrap_or("");
        if labels.len() < 2 || labels.iter().any(|l| l.is_empty()) || tld.len() < 2 || !tld.bytes().all(|x| x.is_ascii_alphabetic()) || NOT_A_TLD.contains(&tld)
        {
            continue;
        }
        if ROLE_LOCALS.contains(&local.to_ascii_lowercase().as_str()) {
            continue;
        }
        let address = format!("{}@{domain_l}", local.to_ascii_lowercase());
        if MAIL_PROVIDERS.contains(&domain_l.as_str()) {
            c.push(ls, at, Repl::Email(address));
        } else {
            c.push(ls, de, Repl::Email(address));
        }
        from = de.max(at + 1);
    }
}

/// Länge eines Pfades ab dem Anfang von `b` (bis Leerraum, Anführungszeichen, Klammern, Trennzeichen)
fn path_run(b: &[u8]) -> usize {
    b.iter()
        .position(|&c| c.is_ascii_whitespace() || matches!(c, b'"' | b'\'' | b'`' | b'<' | b'>' | b'(' | b')' | b'[' | b']' | b'{' | b'}' | b',' | b';' | b'|'))
        .unwrap_or(b.len())
}

fn iso_timestamp(t: &str) -> bool {
    let b = t.as_bytes();
    b.len() >= 19 && b[..4].iter().all(u8::is_ascii_digit) && b[4] == b'-' && b[10] == b'T'
}

fn ip_like(s: &str) -> bool {
    s.parse::<Ipv4Addr>().is_ok() || s.parse::<Ipv6Addr>().is_ok()
}

/// SSIDs: Bereich und Wert (ohne Anführungszeichen).
///
/// Marker `ssid` mit `=`, `:` oder Leerzeichen davor (in Anführungszeichen oder als Wort; ungequotet hinter `:` bis zum
/// Zeilenende, `SSID: My Home Wifi`), wpa_supplicant/NetworkManager `Config: added 'ssid' value '…'` (der Marker steht
/// selbst in Anführungszeichen, der Wert folgt hinter `value`), NetworkManager `access point '…'` und
/// `(wifi) connection '…'`, iwd `Connected to "…"` (nur in Zeilen von iwd). Ein Wert beginnt und endet nie mit Leerraum.
fn ssid_spans(text: &str) -> Vec<(usize, usize, String)> {
    let b = text.as_bytes();
    let lower = text.to_ascii_lowercase();
    let lb = lower.as_bytes();
    let mut out = Vec::new();
    let line_of = |p: usize| {
        let s = b[..p].iter().rposition(|&c| c == b'\n').map_or(0, |x| x + 1);
        let e = p + b[p..].iter().position(|&c| c == b'\n').unwrap_or(b.len() - p);
        (s, e)
    };
    let quoted_after = |marker: &[u8], need_quote: bool, only_in: Option<&[u8]>, out: &mut Vec<(usize, usize, String)>| {
        let mut from = 0;
        while let Some(p) = find(lb, marker, from) {
            from = p + 1;
            if p > 0 && lb[p - 1].is_ascii_alphanumeric() {
                continue;
            }
            let (ls, le) = line_of(p);
            if only_in.is_some_and(|w| find(&lb[ls..le], w, 0).is_none()) {
                continue;
            }
            let mut i = p + marker.len();
            if i < b.len() && b[i].is_ascii_alphanumeric() && !need_quote {
                // `ssidfoo` ist kein Feld
                continue;
            }
            let mut need_quote = need_quote;
            // `'ssid' value 'X'`: das schließende Zeichen des Markers überspringen, der Wert steht hinter `value`
            if p > 0 && matches!(b[p - 1], b'\'' | b'"') && b.get(i) == Some(&b[p - 1]) {
                i += 1;
                while i < b.len() && b[i] == b' ' {
                    i += 1;
                }
                if lb[i..].starts_with(b"value ") {
                    i += 6;
                }
                need_quote = true;
            }
            while i < b.len() && b[i] == b' ' {
                i += 1;
            }
            let mut colon = false;
            if !need_quote && i < b.len() && matches!(b[i], b'=' | b':') {
                colon = b[i] == b':';
                i += 1;
                while i < b.len() && b[i] == b' ' {
                    i += 1;
                }
            }
            let Some(&q) = b.get(i) else { continue };
            let (s, e) = if q == b'\'' || q == b'"' {
                let s = i + 1;
                match b[s..].iter().take(128).position(|&c| c == q || c == b'\n') {
                    Some(n) if b[s + n] == q => (s, s + n),
                    _ => continue,
                }
            } else if need_quote {
                continue;
            } else if colon {
                // `SSID: My Home Wifi`: bis zum Zeilenende
                let mut e = le;
                while e > i && b[e - 1].is_ascii_whitespace() {
                    e -= 1;
                }
                (i, e)
            } else {
                (i, i + b[i..].iter().position(|&c| c.is_ascii_whitespace() || matches!(c, b',' | b';' | b')' | b']')).unwrap_or(b.len() - i))
            };
            let trimmed = e > s && !b[s].is_ascii_whitespace() && !b[e - 1].is_ascii_whitespace();
            if trimmed && e - s <= 128 {
                if let Some(v) = text.get(s..e) {
                    out.push((s, e, v.to_string()));
                }
            }
        }
    };
    quoted_after(b"ssid", false, None, &mut out);
    quoted_after(b"access point ", true, None, &mut out);
    quoted_after(b"(wifi) connection ", true, None, &mut out);
    quoted_after(b"connected to ", true, Some(b"iwd"), &mut out);
    out
}

fn host_suffix_spans(c: &mut Collect) {
    let b = c.b;
    let is_host = |x: u8| x.is_ascii_alphanumeric() || x == b'-' || x == b'.';
    let mut i = 0;
    while i < b.len() {
        if !is_host(b[i]) || (i > 0 && (is_word(b[i - 1]) || matches!(b[i - 1], b'-' | b'.'))) {
            i += 1;
            continue;
        }
        let mut e = i + b[i..].iter().position(|&x| !is_host(x)).unwrap_or(b.len() - i);
        let next = e;
        while e > i && matches!(b[e - 1], b'.' | b'-') {
            e -= 1;
        }
        if let Ok(tok) = std::str::from_utf8(&b[i..e]) {
            let l = tok.to_ascii_lowercase();
            let local = LOCAL_SUFFIXES.iter().any(|s| l.len() > s.len() + 1 && l.ends_with(s) && l.as_bytes()[l.len() - s.len() - 1] == b'.');
            if local && !GENERIC_NAMES.contains(&l.as_str()) && b.get(next).is_none_or(|&x| x != b'_') {
                c.push(i, e, Repl::Name(Category::Host));
            }
        }
        i = next.max(i + 1);
    }
}

fn mac_spans(c: &mut Collect) {
    let b = c.b;
    let is_mac = |x: u8| x.is_ascii_hexdigit() || x == b':';
    let mut i = 0;
    while i < b.len() {
        if !b[i].is_ascii_hexdigit() || (i > 0 && (is_word(b[i - 1]) || matches!(b[i - 1], b':' | b'.'))) {
            i += 1;
            continue;
        }
        let e = i + b[i..].iter().position(|&x| !is_mac(x)).unwrap_or(b.len() - i);
        let run = &b[i..e];
        let groups: Vec<&[u8]> = run.split(|&x| x == b':').collect();
        let after_ok = !b.get(e).is_some_and(|&x| is_word(x) || x == b'.');
        if after_ok && groups.iter().all(|g| g.len() == 2) && [6, 8, 14, 20].contains(&groups.len()) {
            let bytes: Vec<u8> = groups.iter().filter_map(|g| u8::from_str_radix(std::str::from_utf8(g).unwrap_or("zz"), 16).ok()).collect();
            let keep = groups.len() <= 8 && (bytes.iter().all(|&x| x == 0) || bytes.iter().all(|&x| x == 0xff) || bytes[0] & 1 == 1);
            if !keep {
                c.push(i, e, Repl::Mac);
            }
        }
        i = e.max(i + 1);
    }
}

fn keep_v4(a: Ipv4Addr) -> bool {
    let o = a.octets();
    let mask = {
        let v = u32::from(a);
        o[0] == 255 && v.leading_ones() + v.trailing_zeros() == 32
    };
    a.is_unspecified()
        || a.is_loopback()
        || a.is_broadcast()
        || a.is_multicast()
        || mask
        || a.is_documentation()
        || a == Ipv4Addr::new(100, 100, 100, 100)
        || PUBLIC_RESOLVERS.contains(&a.to_string().as_str())
}

fn keep_v6(a: Ipv6Addr) -> bool {
    if let Some(v4) = a.to_ipv4_mapped() {
        return keep_v4(v4);
    }
    let s = a.segments();
    a.is_unspecified()
        || a.is_loopback()
        || a.is_multicast()
        || (s[0] == 0x2001 && s[1] == 0x0db8)
        || a == Ipv6Addr::new(0xfd7a, 0x115c, 0xa1e0, 0, 0, 0, 0, 0x53)
        || PUBLIC_RESOLVERS.contains(&a.to_string().as_str())
}

fn ip_spans(c: &mut Collect) {
    let b = c.b;
    // IPv6 (vor IPv4, damit `::ffff:a.b.c.d` als Ganzes zählt)
    let is_v6 = |x: u8| x.is_ascii_hexdigit() || x == b':' || x == b'.';
    let mut i = 0;
    while i < b.len() {
        if !is_v6(b[i]) || (i > 0 && is_v6(b[i - 1])) {
            i += 1;
            continue;
        }
        let run_end = i + b[i..].iter().position(|&x| !is_v6(x)).unwrap_or(b.len() - i);
        let mut s = i;
        // `addr:fe80::1`: der Doppelpunkt hinter einem Wort trennt
        if s > 0 && (is_word(b[s - 1]) || b[s - 1] == b'.') {
            if b[s] == b':' && b.get(s + 1) != Some(&b':') {
                s += 1;
            } else {
                i = run_end;
                continue;
            }
        }
        let mut e = run_end;
        while e > s && b[e - 1] == b'.' {
            e -= 1;
        }
        if e > s + 1 && b[e - 1] == b':' && b[e - 2] != b':' {
            e -= 1;
        }
        let after_ok = !b.get(run_end).is_some_and(|&x| is_word(x));
        if let Ok(tok) = std::str::from_utf8(&b[s..e]) {
            let colons = tok.bytes().filter(|&x| x == b':').count();
            if after_ok && colons >= 2 && tok.bytes().any(|x| x.is_ascii_digit()) {
                if let Ok(a) = tok.parse::<Ipv6Addr>() {
                    if !keep_v6(a) {
                        c.push(s, e, Repl::Ip(a.to_string()));
                    }
                }
            }
        }
        i = run_end.max(i + 1);
    }
    // IPv4
    let is_v4 = |x: u8| x.is_ascii_digit() || x == b'.';
    let mut i = 0;
    while i < b.len() {
        if !b[i].is_ascii_digit() || (i > 0 && (is_word(b[i - 1]) || b[i - 1] == b'.')) {
            i += 1;
            continue;
        }
        let run_end = i + b[i..].iter().position(|&x| !is_v4(x)).unwrap_or(b.len() - i);
        let mut e = run_end;
        if e > i && b[e - 1] == b'.' {
            e -= 1;
        }
        let after_ok = !b.get(run_end).is_some_and(|&x| is_word(x));
        if after_ok {
            if let Ok(a) = std::str::from_utf8(&b[i..e]).unwrap_or("").parse::<Ipv4Addr>() {
                if !keep_v4(a) {
                    c.push(i, e, Repl::Ip(a.to_string()));
                }
            }
        }
        i = run_end.max(i + 1);
    }
}

/// Erstes Vorkommen von `needle` ab `from`
fn find(hay: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if needle.is_empty() || from >= hay.len() || needle.len() > hay.len() {
        return None;
    }
    (from..=hay.len() - needle.len()).find(|&i| hay[i..i + needle.len()] == *needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blub() -> Pseudonyms {
        Pseudonyms::new(
            Identity {
                user: "blub".into(),
                home: "/home/blub".into(),
                hostname: "blub".into(),
                keep_dirs: vec!["temperature-log".into()],
                ..Default::default()
            },
            true,
        )
    }

    fn other() -> Pseudonyms {
        Pseudonyms::new(Identity { user: "anna".into(), home: "/home/anna".into(), hostname: "werkbank".into(), keep_dirs: vec![], ..Default::default() }, true)
    }

    fn apply(p: &mut Pseudonyms, t: &str) -> String {
        p.apply(t).0
    }

    #[test]
    fn user_home_and_paths() {
        let mut p = other();
        assert_eq!(apply(&mut p, "session opened for user anna(uid=1000) by anna"), "session opened for user user-1(uid=1000) by user-1");
        assert_eq!(apply(&mut p, "open /home/anna/.config/dictate/config.toml: ok"), "open /home/user-1/.config/dictate/config.toml: ok");
        assert_eq!(apply(&mut p, "read /home/anna/Kunden/Müller GmbH/x.pdf"), "read /home/user-1/path-1/path-2 GmbH/x.pdf");
        // Leerzeichen beenden einen Pfad (wie in Logs üblich), Anführungszeichen auch
        assert_eq!(apply(&mut p, "read '/home/anna/Kunden/Müller'"), "read '/home/user-1/path-1/path-2'");
        // gleicher Bestandteil, gleicher Platzhalter; ein anderer Benutzer unter /home bekommt user-2; Endungen bleiben
        assert_eq!(apply(&mut p, "again /home/anna/Kunden/Müller and /home/bert/notes.txt."), "again /home/user-1/path-1/path-2 and /home/user-2/path-3.txt.");
        assert_eq!(apply(&mut p, "/home/anna"), "/home/user-1");
        assert_eq!(apply(&mut p, "the Trash: /home/anna/.local/share/Trash/files/brief.odt"), "the Trash: /home/user-1/.local/share/Trash/files/path-4.odt");
        // ein anderer Ordner unter /home wird nur dort ersetzt, nicht als Wort gelernt (`/home/git`, `/home/data`)
        assert_eq!(apply(&mut p, "bert was here"), "bert was here");
        let mut b = blub();
        assert_eq!(apply(&mut b, "/home/git/blub/dictate and /home/data/stt-test; git data"), "/home/user-2/path-1/path-2 and /home/user-3/path-3; git data");
        // ein Konto aus /etc/passwd ist ein Name, auch als Ordner
        let mut a = Pseudonyms::new(
            Identity { user: "anna".into(), home: "/home/anna".into(), hostname: "werkbank".into(), accounts: vec!["bert".into()], ..Default::default() },
            true,
        );
        assert_eq!(apply(&mut a, "/home/bert/x and bert"), "/home/user-2/path-1 and user-2");
        // nicht unter /home
        assert_eq!(apply(&mut p, "/srv/home/anna2/x and /var/home/x"), "/srv/home/anna2/x and /var/home/x");
        // Teil eines anderen Wortes bleibt; neben `_`, `-`, `.` gilt der Name ab 4 Zeichen (Security-Review M2)
        assert_eq!(apply(&mut p, "annabelle anna_x anna-x"), "annabelle user-1_x user-1-x");
        assert_eq!(p.restore("annabelle user-1_x user-1-x"), "annabelle anna_x anna-x");
        assert_eq!(apply(&mut p, "Anna logged in on werkbank."), "user-1 logged in on host-1.");
        // Ordner aus der Config bleiben
        let mut b = blub();
        assert_eq!(apply(&mut b, "/home/blub/temperature-log/temps-2026-11-04.csv"), "/home/user-1/temperature-log/temps-2026-11-04.csv");
        // Home außerhalb von /home
        let mut h = Pseudonyms::new(
            Identity { user: "svc".into(), home: "/var/lib/svc".into(), hostname: "box1".into(), keep_dirs: vec![], ..Default::default() },
            true,
        );
        assert_eq!(apply(&mut h, "cd /var/lib/svc/data; /var/lib/svcx"), "cd path-1/data; /var/lib/svcx");
    }

    /// Messschritt Diagnose (Aufgabe 10b): technische Bestandteile eines Pfades unter dem Home bleiben (`pgdata` war der
    /// Hinweis in Fall 4), persönliche werden einzeln ersetzt, gleiche mit gleichem Platzhalter.
    #[test]
    fn home_paths_keep_their_technical_parts() {
        let mut p = other();
        let cases = [
            (
                "rsync: send_files failed to open \"/home/anna/projects/dataroom/pgdata/base/16384/2619\": Permission denied",
                "rsync: send_files failed to open \"/home/user-1/projects/path-1/pgdata/base/16384/2619\": Permission denied",
            ),
            ("opendir /home/anna/projects/dataroom/pgdata/pg_stat_tmp failed", "opendir /home/user-1/projects/path-1/pgdata/pg_stat_tmp failed"),
            ("/home/anna/Dokumente/Kunden/Müller/Angebot.odt", "/home/user-1/Dokumente/path-2/path-3/path-4.odt"),
            ("/home/anna/Kunden/Huber/.git/config and /home/anna/Kunden/Müller", "/home/user-1/path-2/path-5/.git/config and /home/user-1/path-2/path-3"),
            ("/home/anna/code/kunde-x/node_modules/sharp/build/Release/sharp.node", "/home/user-1/code/path-6/node_modules/sharp/build/Release/sharp.node"),
            ("/home/anna/docker/kunde-x/compose.yml up", "/home/user-1/docker/path-6/compose.yml up"),
            ("/home/anna/.local/share/docker/volumes/x/_data", "/home/user-1/.local/share/docker/volumes/x/_data"),
            ("/home/anna/Downloads/Steuer 2025.pdf", "/home/user-1/Downloads/path-7 2025.pdf"),
            ("/home/anna/Bilder/Urlaub.tar.gz", "/home/user-1/Bilder/path-8.tar.gz"),
            ("/home/anna/.cache/thumbnails/large/0f3a.png", "/home/user-1/.cache/thumbnails/large/path-9.png"),
            ("{\"path\":\"\\/home\\/anna\\/Kunden\\/Müller\\/pgdata\"}", "{\"path\":\"\\/home\\/user-1\\/path-2\\/path-3\\/pgdata\"}"),
        ];
        let mut all = String::new();
        for (input, want) in cases {
            let got = apply(&mut p, input);
            assert_eq!(got, want, "{input}");
            assert_eq!(p.restore(&got), input, "{input}");
            all.push_str(&got);
        }
        for name in ["anna", "dataroom", "Kunden", "Müller", "Huber", "kunde-x", "Angebot", "Steuer", "Urlaub", "0f3a"] {
            assert!(!all.contains(name), "{name} in {all}");
        }
    }

    #[test]
    fn user_and_host_with_the_same_name_share_one_placeholder() {
        let mut p = blub();
        let line = "2026-11-04T09:46:55+0100 blub kernel: NVRM: Xid 79 on blub; HOME=/home/blub";
        p.learn("boot_end", line);
        assert_eq!(apply(&mut p, line), "2026-11-04T09:46:55+0100 user-1 kernel: NVRM: Xid 79 on user-1; HOME=/home/user-1");
        assert_eq!(p.restore("user-1 is host and user"), "blub is host and user");
    }

    #[test]
    fn hosts_learned_from_the_structure_of_probes() {
        let mut p = other();
        p.learn("boot_end", "2026-11-04T09:46:55+0100 werkbank systemd[1]: Started x.\n2026-11-04T09:46:56+0100 lager-7 kernel: y");
        let ts = "backend: Running  version: 1.76.1\nhealth: no warnings\nself: werkbank (werkbank.tail4f2a.ts.net) online=yes relay=fra os=linux\npeers: 2 (1 online)\npeer           online relay last seen  exit node\nAnnas-Pixel-8  yes    fra   -          no\nnas            no     fra   2026-11-01 no\n";
        // `net_tailscale` und `net_mounts` melden ihre Namen strukturiert (`ProbeOutput::names`), der Text lehrt nichts
        p.learn("net_tailscale", ts);
        p.learn("net_mounts", "target type source options\n/mnt/a cifs //fileserver/share rw\n/mnt/b nfs4 vault.example.org:/export rw");
        assert_eq!(apply(&mut p, "Annas-Pixel-8 fileserver"), "Annas-Pixel-8 fileserver");
        let names = |v: &[(Category, &str)]| v.iter().map(|(c, n)| (*c, n.to_string())).collect::<Vec<_>>();
        p.learn_names(&names(&[
            (Category::Host, "werkbank"),
            (Category::Host, "werkbank.tail4f2a.ts.net"),
            (Category::Host, "Annas-Pixel-8"),
            (Category::Host, "nas"),
            (Category::Host, "Daniel's iPad"),
            (Category::Host, "fileserver"),
            (Category::Path, "//fileserver/share"),
            (Category::Host, "vault.example.org"),
            (Category::User, "dkurd"),
        ]));
        p.learn("net_reach", "PING printer-og (192.168.1.40) 56(84) bytes of data.");
        p.learn("docker_logs", "2026-11-04T09:46:55.000Z notahost started"); // Docker-Zeilen haben keine Rechnerspalte
        let (out, counts) = p.apply(ts);
        assert!(out.contains("self: host-1 (host-2) online=yes"), "{out}");
        assert!(out.contains("\nhost-3  yes"), "{out}");
        // `nas` ist allgemein und bleibt
        assert!(out.contains("\nnas            no"), "{out}");
        assert_eq!(counts, [(Category::Host, 3)]);
        assert_eq!(apply(&mut p, "lager-7 mounts //fileserver/share from vault.example.org"), "host-4 mounts path-1 from host-5");
        assert_eq!(apply(&mut p, "printer-og answered, notahost did not"), "host-6 answered, notahost did not");
        // Namen mit Leerzeichen und Apostroph als Ganzes, entfernte Benutzer von sshfs
        assert_eq!(apply(&mut p, "peer Daniel's iPad offline; dkurd@vault.example.org:/srv"), "peer host-7 offline; user-2@host-5:/srv");
        assert_eq!(p.restore("host-7 user-2"), "Daniel's iPad dkurd");
        // Groß-/Kleinschreibung egal
        assert_eq!(apply(&mut p, "WERKBANK"), "host-1");
    }

    #[test]
    fn local_suffixes_and_generic_names() {
        let mut p = other();
        assert_eq!(apply(&mut p, "DNS Domain: tail4f2a.ts.net ~."), "DNS Domain: host-2 ~.");
        assert_eq!(apply(&mut p, "resolved nas.fritz.box, drucker.local. and box.home.arpa"), "resolved host-3, host-4. and host-5");
        assert_eq!(apply(&mut p, "ts.net localhost localhost.localdomain example.com"), "ts.net localhost localhost.localdomain example.com");
    }

    #[test]
    fn ip_addresses() {
        let mut p = other();
        let t = "inet 192.168.1.23/24 brd 192.168.1.255 via 192.168.1.1 dev enp5s0; from 192.168.1.23:52344";
        assert_eq!(apply(&mut p, t), "inet ip-1/24 brd ip-2 via ip-3 dev enp5s0; from ip-1:52344");
        // bleiben: Loopback, unspezifiziert, Broadcast, Netzmasken, Multicast, Doku, MagicDNS, öffentliche Resolver
        let keep = "127.0.0.1 127.0.0.53 0.0.0.0 255.255.255.255 255.255.255.0 224.0.0.251 192.0.2.7 100.100.100.100 1.1.1.1 8.8.8.8 9.9.9.9 :: ::1 ff02::fb 2001:db8::5 fd7a:115c:a1e0::53 2606:4700:4700::1111 ::ffff:127.0.0.1";
        assert_eq!(apply(&mut p, keep), keep);
        // ersetzt: öffentlich, CGNAT/Tailscale, Link-local, IPv6 global und Link-local, IPv4-mapped
        assert_eq!(
            apply(&mut p, "peer 100.101.102.103 wan 84.112.3.9 ll 169.254.3.1 v6 2a02:8388:1:2::5 fe80::1ff:fe23:4567:890a%enp5s0 m ::ffff:10.0.0.5"),
            "peer ip-4 wan ip-5 ll ip-6 v6 ip-7 ip-8%enp5s0 m ip-9"
        );
        // Schreibweisen derselben Adresse sind ein Wert
        assert_eq!(apply(&mut p, "2A02:8388:0001:0002:0000:0000:0000:0005 [2a02:8388:1:2::5]:443 addr:fe80::1ff:fe23:4567:890a"), "ip-7 [ip-7]:443 addr:ip-8");
        // keine Adressen: Versionen, Zeiten, PCI, Rust-Pfade, Hex ohne Ziffer, Teile von Wörtern
        let not = "v1.2.3.4 6.11.5.arch1-1 1.2.3.4.5 10:00:00 12:34 0000:01:00.0 std::io dead::beef x192.168.1.1 192.168.1.1x 300.1.1.1";
        assert_eq!(apply(&mut p, not), not);
        // Satzende
        assert_eq!(apply(&mut p, "reach 10.0.0.9."), "reach ip-10.");
    }

    #[test]
    fn mac_addresses() {
        let mut p = other();
        assert_eq!(apply(&mut p, "link/ether 3c:7c:3f:1a:2b:4d brd ff:ff:ff:ff:ff:ff"), "link/ether mac-1 brd ff:ff:ff:ff:ff:ff");
        assert_eq!(
            apply(&mut p, "mac 3C:7C:3F:1A:2B:4D 00:00:00:00:00:00 01:00:5e:00:00:fb 33:33:00:00:00:01"),
            "mac mac-1 00:00:00:00:00:00 01:00:5e:00:00:fb 33:33:00:00:00:01"
        );
        // Netfilter `MAC=` (Ziel, Quelle, Typ) als Ganzes
        assert_eq!(apply(&mut p, "MAC=ff:ff:ff:ff:ff:ff:3c:7c:3f:1a:2b:4d:08:00 SRC=10.1.2.3"), "MAC=mac-2 SRC=ip-1");
        // keine MAC: Zeiten, kürzere oder längere Gruppen, Fingerabdrücke mit 16 Byte
        let not = "12:34:56 aa:bb:cc:dd:ee 0a:1b:2c:3d:4e:5f:60:71:82:93:a4:b5:c6:d7:e8:f9 3c:7c:3f:1a:2b:4dz";
        assert_eq!(apply(&mut p, not), not);
    }

    #[test]
    fn ssids() {
        let mut p = other();
        p.learn("kernel_errors", "wlan0: Trying to associate with SSID 'Haus Müller 5G'");
        assert_eq!(
            apply(&mut p, "NetworkManager: Activation: (wifi) access point 'Haus Müller 5G' has security"),
            "NetworkManager: Activation: (wifi) access point 'ssid-1' has security"
        );
        assert_eq!(apply(&mut p, "ssid=CafeNet, ssid: \"Other Net\" BSSID 3c:7c:3f:1a:2b:4d"), "ssid=ssid-2, ssid: \"ssid-3\" BSSID mac-1");
        assert_eq!(apply(&mut p, "connected to Haus Müller 5G again; CafeNet"), "connected to ssid-1 again; ssid-2");
        assert_eq!(apply(&mut p, "(wifi) connection 'Büro' failed"), "(wifi) connection 'ssid-4' failed");
        assert_eq!(apply(&mut p, "ssidx=1 the bssid list"), "ssidx=1 the bssid list");
    }

    #[test]
    fn stable_across_sections_and_rounds_and_restorable() {
        let mut p = other();
        let a = apply(&mut p, "host werkbank at 192.168.5.5, mac 3c:7c:3f:1a:2b:4d, /home/anna/x.txt");
        let b = apply(&mut p, "again 192.168.5.5 and 3c:7c:3f:1a:2b:4d on werkbank, /home/anna/x.txt");
        assert_eq!(a, "host host-1 at ip-1, mac mac-1, /home/user-1/path-1.txt");
        assert_eq!(b, "again ip-1 and mac-1 on host-1, /home/user-1/path-1.txt");
        assert_eq!(
            p.restore("Check ip-1 (mac-1) on host-1, see /home/user-1/path-1.txt; ip-99 is unknown, myhost-1x stays"),
            "Check 192.168.5.5 (3c:7c:3f:1a:2b:4d) on werkbank, see /home/anna/x.txt; ip-99 is unknown, myhost-1x stays"
        );
    }

    #[test]
    fn placeholders_in_the_raw_text_never_collide() {
        let mut p = other();
        // Rohtext enthält schon Platzhalter-Formen: sie werden selbst ersetzt
        // vergeben sind user-1 (anna) und host-1 (werkbank); ip-3, host-2, path-7 sind es nicht und bleiben
        let raw = "user-1 says hi from host-1 and ip-3; my-host-2 rack; user-01 user-1x path-7.";
        let (out, _) = p.apply(raw);
        assert_eq!(out, "user-2 says hi from host-3 and ip-3; my-host-2 rack; user-01 user-2x path-7.");
        // jeder Platzhalter im Ergebnis bedeutet genau einen Wert, die Rückübersetzung ergibt den Rohtext
        assert_eq!(p.restore(&out), raw);
        let mixed = apply(&mut p, "anna on werkbank vs user-1 on host-1");
        assert_eq!(mixed, "user-1 on host-1 vs user-2 on host-3");
        assert_eq!(p.restore(&mixed), "anna on werkbank vs user-1 on host-1");
        // reservierte Formen werden nie vergeben: neue Adressen überspringen ip-3
        assert_eq!(apply(&mut p, "10.0.0.1 10.0.0.2 10.0.0.3"), "ip-1 ip-2 ip-4");
        assert_eq!(apply(&mut p, "user-1000.slice"), "user-1000.slice");
        // eine spätere Runde enthält einen vergebenen Platzhalter als Rohtext: er wird ersetzt
        let (out, _) = p.apply("raw ip-4 seen");
        assert_ne!(out, "raw ip-4 seen");
        assert_eq!(p.restore(&out), "raw ip-4 seen");
        // Kein Platzhalter im Ergebnis zeigt auf zwei Werte
        let mut seen = HashMap::new();
        for (k, v) in &p.forward {
            assert!(seen.insert(v.clone(), k.clone()).is_none(), "{v} twice");
        }
    }

    #[test]
    fn redactions_stay_and_disabled_changes_nothing() {
        let mut p = other();
        assert_eq!(apply(&mut p, "token=[REDACTED:jwt] for anna"), "token=[REDACTED:jwt] for user-1");
        let mut off = Pseudonyms::new(
            Identity { user: "anna".into(), home: "/home/anna".into(), hostname: "werkbank".into(), keep_dirs: vec![], ..Default::default() },
            false,
        );
        let t = "anna on werkbank 192.168.1.2 3c:7c:3f:1a:2b:4d /home/anna/x";
        off.learn("boot_end", t);
        assert_eq!(off.apply(t), (t.to_string(), vec![]));
        assert_eq!(off.restore("user-1"), "user-1");
        assert!(!off.enabled());
    }

    #[test]
    fn debug_never_shows_the_mapping() {
        let mut p = other();
        let _ = p.apply("anna 192.168.1.2");
        let d = format!("{p:?}");
        assert!(!d.contains("anna") && !d.contains("192.168") && !d.contains("werkbank"), "{d}");
    }

    #[test]
    fn utf8_and_odd_input_without_panics() {
        let mut p = blub();
        for s in
            ["ä/home/ä/ö", "/home/", "/home//x", "ssid='", "ssid=\"ä", "fe80::ä", "1.2.3.ä", "äblubä", "[REDACTED:", "user-", "host-0", "€:€:€", "::", ":", "."]
        {
            let (out, _) = p.apply(s);
            assert_eq!(p.restore(&out), s, "{s:?}");
            for cut in 0..=s.len() {
                if let Some(h) = s.get(..cut) {
                    let (o, _) = p.apply(h);
                    assert_eq!(p.restore(&o), h);
                }
            }
        }
    }

    /// Security-Review M2: Namen neben `-`, `_`, `.` und `\x2d` (Units aus Pfaden, LVM, Compose, Rechnernamen).
    #[test]
    fn names_next_to_separators() {
        let mut p = blub();
        for (raw, want) in [
            ("home-blub-nas.mount failed", "home-user-1-nas.mount failed"),
            ("/dev/mapper/blub--vg-root", "/dev/mapper/user-1--vg-root"),
            ("container blub_db_1 exited", "container user-1_db_1 exited"),
            ("run-media-blub-Stick.mount", "run-media-user-1-Stick.mount"),
            (r"systemd-fsck@dev-mapper-blub\x2dvg.service", r"systemd-fsck@dev-mapper-user-1\x2dvg.service"),
            (r"dev-mapper-vg\x2dblub.device", r"dev-mapper-vg\x2duser-1.device"),
            ("from blub-laptop.", "from user-1-laptop."),
            ("blubber blub2 2blub", "blubber blub2 2blub"),
        ] {
            let out = apply(&mut p, raw);
            assert_eq!(out, want, "{raw}");
            assert_eq!(p.restore(&out), raw);
        }
        // kurze Namen (unter 4 Zeichen) nur allein
        let mut s = Pseudonyms::new(Identity { user: "bo".into(), home: "/home/bo".into(), hostname: "pc7".into(), ..Default::default() }, true);
        assert_eq!(apply(&mut s, "bo-x pc7_y bo pc7"), "bo-x pc7_y user-1 host-1");
    }

    /// Security-Review M3: `file://`-URIs, `//home/`, JSON-escapte Pfade.
    #[test]
    fn paths_in_uris_and_escaped_json() {
        let mut p = other();
        assert_eq!(
            apply(&mut p, "Failed to open file:///home/anna/Projekte/Kunde%20Meier/Angebot.pdf"),
            "Failed to open file:///home/user-1/Projekte/path-1/path-2.pdf"
        );
        assert_eq!(apply(&mut p, "//home/anna/Projekte/x"), "//home/user-1/Projekte/path-3");
        assert_eq!(
            apply(&mut p, r#"{"path":"\/home\/anna\/Projekte\/x","cfg":"\/home\/anna\/.config\/a"}"#),
            r#"{"path":"\/home\/user-1\/Projekte\/path-3","cfg":"\/home\/user-1\/.config\/a"}"#
        );
        assert_eq!(p.restore("file:///home/user-1/Projekte/path-1/path-2.pdf"), "file:///home/anna/Projekte/Kunde%20Meier/Angebot.pdf");
        // weiterhin nicht: ein `/home/` mitten in einem anderen Pfad
        assert_eq!(apply(&mut p, "/srv//home/x /data/home/y"), "/srv//home/x /data/home/y");
    }

    /// Security-Review M4: E-Mail-Adressen und strukturell erkannte Benutzernamen.
    #[test]
    fn emails_and_learned_users() {
        let mut p = other();
        assert_eq!(apply(&mut p, "user daniel.k@firma.at logged in, copy to anna.b+x@gmail.com"), "user email-1 logged in, copy to email-2@gmail.com");
        assert_eq!(p.restore("email-1 email-2@gmail.com"), "daniel.k@firma.at anna.b+x@gmail.com");
        // bleiben: Units, Pakete, Images, Rollen, Benutzer an einem Rechner (sshfs, git, URL)
        let keep = "getty@tty1.service user@1000.service image@sha256:abcd git@github.com noreply@example.org linux@archlinux pkg@1.2.3";
        assert_eq!(apply(&mut p, keep), keep);
        p.learn(
            "unit_journal",
            "2026-11-05T10:00:00+0100 werkbank sshd[1]: Accepted publickey for bert from 10.0.0.2 port 5 ssh2\n\
             2026-11-05T10:00:01+0100 werkbank sshd[1]: pam_unix(sshd:session): session opened for user carla(uid=1001) by (uid=0)\n\
             2026-11-05T10:00:02+0100 werkbank sudo[2]: pam_unix(sudo:auth): authentication failure; logname=dora uid=1002 euid=0 tty=/dev/pts/1 ruser=dora rhost=  user=dora\n\
             2026-11-05T10:00:03+0100 werkbank sshd[3]: Invalid user eve from 192.0.2.9 port 4\n\
             2026-11-05T10:00:04+0100 werkbank systemd-logind[4]: New session 3 of user gdm.\n\
             2026-11-05T10:00:05+0100 werkbank sshd[5]: session opened for user polkitd(uid=102) by (uid=0); USER=root",
        );
        assert_eq!(apply(&mut p, "bert carla dora eve gdm polkitd root"), "user-2 user-3 user-4 user-5 gdm polkitd root");
    }

    /// Security-Review M5: SSIDs in den Meldungen von wpa_supplicant, NetworkManager und iwd.
    #[test]
    fn ssid_markers_of_supplicant_and_iwd() {
        let mut p = other();
        let line = "NetworkManager[9]: <info> device (wlan0): supplicant: Config: added 'ssid' value 'Heimnetz Meier'";
        assert_eq!(apply(&mut p, line), "NetworkManager[9]: <info> device (wlan0): supplicant: Config: added 'ssid' value 'ssid-1'");
        assert_eq!(apply(&mut p, "added 'key_mgmt' value 'WPA-PSK' and ' value '"), "added 'key_mgmt' value 'WPA-PSK' and ' value '");
        assert_eq!(apply(&mut p, "wlan0: Trying to associate with SSID 'Cafe Linde'"), "wlan0: Trying to associate with SSID 'ssid-2'");
        assert_eq!(apply(&mut p, "iwd[7]: Connected to \"Büro 2\""), "iwd[7]: Connected to \"ssid-3\"");
        assert_eq!(apply(&mut p, "dbus: Connected to \"session bus\""), "dbus: Connected to \"session bus\"");
        assert_eq!(apply(&mut p, "SSID: My Home Wifi\nnext"), "SSID: ssid-4\nnext");
    }

    /// Security-Review N2, N3: nur echte Schwärzungen schützen; Platzhalter-Formen in Wörtern blockieren keine Funde.
    #[test]
    fn redaction_markers_and_inner_placeholder_forms() {
        let mut p = other();
        assert_eq!(apply(&mut p, "[REDACTED: oops\nfrom 192.168.1.5 by anna]"), "[REDACTED: oops\nfrom ip-1 by user-1]");
        assert_eq!(apply(&mut p, "[REDACTED:jwt] 192.168.1.5"), "[REDACTED:jwt] ip-1");
        assert_eq!(apply(&mut p, "node ip-10-0-0-5.eu-central-1.compute.internal up"), "node host-2 up");
        assert_eq!(apply(&mut p, "user-7.fritz.box"), "host-3");
        assert_eq!(p.restore("host-2 host-3"), "ip-10-0-0-5.eu-central-1.compute.internal user-7.fritz.box");
        // eine allein stehende Form bleibt reserviert
        assert_eq!(apply(&mut p, "user-1000.slice ip-7"), "user-1000.slice ip-7");
    }

    /// Security-Review N6: die Machine-ID ist bekannt und wird exakt ersetzt, auch in UUID-Schreibweise.
    #[test]
    fn machine_id_is_replaced() {
        let id = "4f1d0c9e8b7a46e5a3c2b1d0e9f8a7b6";
        let mut p = Pseudonyms::new(
            Identity { user: "anna".into(), home: "/home/anna".into(), hostname: "werkbank".into(), machine_id: id.into(), ..Default::default() },
            true,
        );
        assert_eq!(
            apply(&mut p, "/var/log/journal/4f1d0c9e8b7a46e5a3c2b1d0e9f8a7b6/system.journal MACHINE_ID=4F1D0C9E-8B7A-46E5-A3C2-B1D0E9F8A7B6 x4f1d0c9e8b7a46e5a3c2b1d0e9f8a7b6"),
            "/var/log/journal/id-1/system.journal MACHINE_ID=id-1 x4f1d0c9e8b7a46e5a3c2b1d0e9f8a7b6"
        );
        assert_eq!(Category::Id.describe(), "Machine ID");
        assert_eq!(Category::Email.prefix(), "email");
    }
}
