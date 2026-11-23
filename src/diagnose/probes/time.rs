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

//! Zeit für die Proben: Kalenderrechnung ohne Zusatz-Crate, Lesen und Schreiben von ISO-Zeitstempeln und die Uhr
//! ([`Clock`]), die Tests festlegen können (Jetzt und Zeitzone).
//!
//! Das Journal liefert Zeiten als Mikrosekunden seit der Epoche (UTC) und `short-iso`-Zeilen mit Versatz
//! (`2026-11-04T09:47:12+0100`); der temp-logger schreibt Ortszeit ohne Versatz (`2026-11-04 09:47:12`). Zum Umrechnen
//! der Ortszeit braucht es den Versatz *zu der Zeit* (Sommerzeit): [`Clock::utc_offset_at`] fragt in Produktion die
//! C-Bibliothek (`localtime_r`), in Tests ist er fest.

/// Sekunden pro Tag.
pub const DAY: i64 = 86_400;

/// Tage seit 1970-01-01 für ein Datum im proleptischen gregorianischen Kalender (Howard Hinnant, `days_from_civil`).
pub fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = i64::from((m + 9) % 12);
    let doy = (153 * mp + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Datum zu Tagen seit 1970-01-01 (Umkehrung von [`days_from_civil`]).
pub fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Kalenderfelder einer Zeit in einer Zone mit festem Versatz.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Civil {
    pub year: i64,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
}

impl Civil {
    /// Die Felder zur Zeit `epoch` (Sekunden seit der Epoche, UTC) in einer Zone mit `offset` Sekunden östlich von UTC.
    pub fn at(epoch: i64, offset: i32) -> Civil {
        let local = epoch + i64::from(offset);
        let days = local.div_euclid(DAY);
        let rest = local.rem_euclid(DAY);
        let (year, month, day) = civil_from_days(days);
        Civil { year, month, day, hour: (rest / 3600) as u32, minute: (rest % 3600 / 60) as u32, second: (rest % 60) as u32 }
    }

    /// Sekunden seit der Epoche, wenn die Felder UTC wären (der Versatz ist noch nicht abgezogen).
    pub fn as_utc_secs(&self) -> i64 {
        days_from_civil(self.year, self.month, self.day) * DAY + i64::from(self.hour) * 3600 + i64::from(self.minute) * 60 + i64::from(self.second)
    }

    /// `2026-11-04` (der Tag, wie ihn der temp-logger im Dateinamen führt).
    pub fn date(&self) -> String {
        format!("{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

/// `2026-11-04T09:47:12+0100`, die Form von `journalctl -o short-iso` (Versatz in Sekunden östlich von UTC).
pub fn format_iso(epoch: i64, offset: i32) -> String {
    let c = Civil::at(epoch, offset);
    let sign = if offset < 0 { '-' } else { '+' };
    let off = offset.unsigned_abs();
    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}{sign}{:02}{:02}", c.year, c.month, c.day, c.hour, c.minute, c.second, off / 3600, off % 3600 / 60)
}

/// Zahl aus genau `n` Ziffern am Anfang von `s`; der Rest folgt.
fn digits(s: &str, n: usize) -> Option<(u32, &str)> {
    let head = s.get(..n)?;
    if !head.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some((head.parse().ok()?, &s[n..]))
}

fn valid(c: &Civil) -> bool {
    (1..=12).contains(&c.month) && (1..=31).contains(&c.day) && c.hour < 24 && c.minute < 60 && c.second < 61
}

/// Liest `YYYY-MM-DD` + `T` oder Leerzeichen + `HH:MM:SS` am Anfang und liefert die Felder und den Rest.
fn civil_prefix(s: &str) -> Option<(Civil, &str)> {
    let (year, s) = digits(s, 4)?;
    let s = s.strip_prefix('-')?;
    let (month, s) = digits(s, 2)?;
    let s = s.strip_prefix('-')?;
    let (day, s) = digits(s, 2)?;
    let s = s.strip_prefix('T').or_else(|| s.strip_prefix(' '))?;
    let (hour, s) = digits(s, 2)?;
    let s = s.strip_prefix(':')?;
    let (minute, s) = digits(s, 2)?;
    let s = s.strip_prefix(':')?;
    let (second, s) = digits(s, 2)?;
    let c = Civil { year: i64::from(year), month, day, hour, minute, second };
    valid(&c).then_some((c, s))
}

/// Versatz hinter einem Zeitstempel: `Z`, `+0100`, `+01:00` oder `+01`; `None` ohne Versatz. Der Rest folgt.
fn offset_suffix(s: &str) -> Option<(i32, &str)> {
    if let Some(rest) = s.strip_prefix('Z') {
        return Some((0, rest));
    }
    let sign = match s.chars().next()? {
        '+' => 1,
        '-' => -1,
        _ => return None,
    };
    let s = &s[1..];
    let (h, s) = digits(s, 2)?;
    let (m, s) = match s.strip_prefix(':') {
        Some(t) => digits(t, 2).unwrap_or((0, t)),
        None => digits(s, 2).unwrap_or((0, s)),
    };
    Some((sign * (h as i32 * 3600 + m as i32 * 60), s))
}

/// Ein Zeitstempel mit Versatz am Anfang (`2026-11-04T09:47:12+0100`, auch mit Bruchteil `.123456` und mit `Z`):
/// Sekunden seit der Epoche und der Versatz. Ohne Versatz `None` (Ortszeit liest [`Clock::parse_local`]).
pub fn parse_iso_prefix(s: &str) -> Option<(i64, i32, &str)> {
    let (civil, mut rest) = civil_prefix(s)?;
    if let Some(frac) = rest.strip_prefix('.') {
        let n = frac.bytes().take_while(u8::is_ascii_digit).count();
        if n == 0 {
            return None;
        }
        rest = &frac[n..];
    }
    let (offset, rest) = offset_suffix(rest)?;
    Some((civil.as_utc_secs() - i64::from(offset), offset, rest))
}

/// Wie [`parse_iso_prefix`], nur die Sekunden.
pub fn parse_iso(s: &str) -> Option<i64> {
    parse_iso_prefix(s).map(|(secs, _, _)| secs)
}

/// Welche Zone die Uhr benutzt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Zone {
    /// Die Ortszeit des Systems (`localtime_r`).
    Local,
    /// Fester Versatz in Sekunden östlich von UTC (Tests).
    Fixed(i32),
}

/// Die Uhr der Proben: das Jetzt und die Zone der Ortszeit. Die Proben fragen nie die Systemzeit direkt, damit Tests
/// ein festes Jetzt und eine feste Zone setzen können.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Clock {
    now: Option<i64>,
    zone: Zone,
}

impl Clock {
    /// Die Systemuhr mit der Ortszeit des Systems.
    pub fn system() -> Clock {
        Clock { now: None, zone: Zone::Local }
    }

    /// Feste Uhr für Tests: `now` Sekunden seit der Epoche, Zone mit festem Versatz.
    pub fn fixed(now: i64, offset: i32) -> Clock {
        Clock { now: Some(now), zone: Zone::Fixed(offset) }
    }

    /// Jetzt, Sekunden seit der Epoche.
    pub fn now(&self) -> i64 {
        self.now.unwrap_or_else(|| std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0))
    }

    /// Versatz der Ortszeit zur Zeit `epoch`, Sekunden östlich von UTC.
    pub fn utc_offset_at(&self, epoch: i64) -> i32 {
        match self.zone {
            Zone::Fixed(offset) => offset,
            Zone::Local => local_offset(epoch),
        }
    }

    /// `2026-11-04T09:47:12+0100` in der Ortszeit.
    pub fn format(&self, epoch: i64) -> String {
        format_iso(epoch, self.utc_offset_at(epoch))
    }

    /// Die Kalenderfelder in der Ortszeit.
    pub fn civil(&self, epoch: i64) -> Civil {
        Civil::at(epoch, self.utc_offset_at(epoch))
    }

    /// Ortszeit ohne Versatz (`2026-11-04 09:47:12`, auch mit `T`) als Sekunden seit der Epoche. Beim Wechsel der
    /// Sommerzeit gilt der Versatz zur gefundenen Zeit.
    pub fn parse_local(&self, s: &str) -> Option<i64> {
        let (civil, rest) = civil_prefix(s.trim())?;
        if !rest.is_empty() {
            return None;
        }
        let wall = civil.as_utc_secs();
        let first = wall - i64::from(self.utc_offset_at(wall));
        let second = wall - i64::from(self.utc_offset_at(first));
        Some(second)
    }
}

/// Versatz der Ortszeit des Systems zur Zeit `epoch` über `localtime_r` (thread-sicher; die C-Bibliothek liest die
/// Zeitzone beim ersten Aufruf).
fn local_offset(epoch: i64) -> i32 {
    let t: libc::time_t = epoch as libc::time_t;
    // SAFETY: `tm` ist ein reiner Datensatz, den `localtime_r` vollständig füllt; beide Zeiger sind gültig.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    let ok = unsafe { !libc::localtime_r(&t, &mut tm).is_null() };
    if ok {
        tm.tm_gmtoff as i32
    } else {
        0
    }
}

/// Dauer als kurzer Text: `45 s`, `12 min`, `3 h 05 min`, `2 d 04 h`.
pub fn format_duration(secs: i64) -> String {
    let secs = secs.max(0);
    if secs < 120 {
        format!("{secs} s")
    } else if secs < 2 * 3600 {
        format!("{} min", secs / 60)
    } else if secs < 2 * DAY {
        format!("{} h {:02} min", secs / 3600, secs % 3600 / 60)
    } else {
        format!("{} d {:02} h", secs / DAY, secs % DAY / 3600)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calendar_round_trips_over_many_years() {
        let mut days = days_from_civil(1999, 1, 1);
        let end = days_from_civil(2102, 1, 1);
        let mut prev = civil_from_days(days - 1);
        while days < end {
            let (y, m, d) = civil_from_days(days);
            assert_eq!(days_from_civil(y, m, d), days, "{y}-{m}-{d}");
            // Der Tag folgt auf den vorigen
            assert!((y, m, d) > prev, "{prev:?} then {:?}", (y, m, d));
            prev = (y, m, d);
            days += 1;
        }
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(2026, 11, 4), 20_761);
        assert_eq!(civil_from_days(20_761), (2026, 11, 4));
        // Schaltjahre
        assert_eq!(civil_from_days(days_from_civil(2024, 2, 29)), (2024, 2, 29));
        assert_eq!(civil_from_days(days_from_civil(2100, 2, 28) + 1), (2100, 3, 1));
        assert_eq!(civil_from_days(days_from_civil(2000, 2, 28) + 1), (2000, 2, 29));
    }

    #[test]
    fn iso_timestamps_with_offsets_parse_and_format() {
        // 2026-11-04T09:47:12+0100 ist 08:47:12 UTC
        let utc = days_from_civil(2026, 11, 4) * DAY + 8 * 3600 + 47 * 60 + 12;
        assert_eq!(parse_iso("2026-11-04T09:47:12+0100"), Some(utc));
        assert_eq!(parse_iso("2026-11-04T09:47:12+01:00"), Some(utc));
        assert_eq!(parse_iso("2026-11-04T09:47:12+01"), Some(utc));
        assert_eq!(parse_iso("2026-11-04T08:47:12Z"), Some(utc));
        assert_eq!(parse_iso("2026-11-04T08:47:12.123456789Z"), Some(utc));
        assert_eq!(parse_iso("2026-11-04T03:17:12-0530"), Some(utc));
        let (secs, off, rest) = parse_iso_prefix("2026-11-04T09:47:12+0100 host kernel: x").unwrap();
        assert_eq!((secs, off, rest), (utc, 3600, " host kernel: x"));
        assert_eq!(format_iso(utc, 3600), "2026-11-04T09:47:12+0100");
        assert_eq!(format_iso(utc, 0), "2026-11-04T08:47:12+0000");
        assert_eq!(format_iso(utc, -19_800), "2026-11-04T03:17:12-0530");
        // Kein Zeitstempel
        for bad in [
            "",
            "hello",
            "2026-11-04",
            "2026-11-04T09:47",
            "2026-13-04T09:47:12+0100",
            "2026-11-04T25:47:12+0100",
            "2026-11-04 09:47:12",
            "2026-11-04T09:47:12",
            "2026-11-04T09:47:12.+0100",
        ] {
            assert_eq!(parse_iso(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn local_time_uses_the_zone_of_the_clock() {
        let clock = Clock::fixed(0, 3600);
        let utc = days_from_civil(2026, 11, 4) * DAY + 8 * 3600 + 47 * 60 + 12;
        assert_eq!(clock.parse_local("2026-11-04 09:47:12"), Some(utc));
        assert_eq!(clock.parse_local("2026-11-04T09:47:12"), Some(utc));
        assert_eq!(clock.parse_local(" 2026-11-04 09:47:12 "), Some(utc));
        assert_eq!(clock.parse_local("2026-11-04 09:47:12+0100"), None);
        assert_eq!(clock.parse_local("nope"), None);
        assert_eq!(clock.format(utc), "2026-11-04T09:47:12+0100");
        assert_eq!(clock.civil(utc).date(), "2026-11-04");
        // Kurz nach Mitternacht Ortszeit ist noch der Vortag in UTC
        let late = days_from_civil(2026, 11, 5) * DAY + 30;
        assert_eq!(Clock::fixed(0, 3600).civil(late).date(), "2026-11-05");
        assert_eq!(Clock::fixed(0, 0).civil(late - 60).date(), "2026-11-04");
        assert_eq!(Clock::fixed(77, 0).now(), 77);
        assert!(Clock::system().now() > 1_700_000_000);
    }

    #[test]
    fn the_system_zone_gives_a_plausible_offset() {
        // Nicht die Zone des Rechners prüfen (sie ist bekannt, aber der Test soll überall laufen), nur die Spanne
        let off = Clock::system().utc_offset_at(1_800_000_000);
        assert!((-14 * 3600..=14 * 3600).contains(&off), "{off}");
    }

    #[test]
    fn durations_read_well() {
        assert_eq!(format_duration(-5), "0 s");
        assert_eq!(format_duration(45), "45 s");
        assert_eq!(format_duration(600), "10 min");
        assert_eq!(format_duration(3 * 3600 + 300), "3 h 05 min");
        assert_eq!(format_duration(3 * DAY + 4 * 3600), "3 d 04 h");
    }
}
