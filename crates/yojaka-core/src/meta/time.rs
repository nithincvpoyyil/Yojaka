//! Tiny calendar helpers: wall-clock text ↔ unix seconds, with no timezone applied.

/// Days since 1970-01-01 for a proleptic Gregorian date (Howard Hinnant's algorithm).
pub fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

pub(crate) fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

pub(crate) fn to_secs(y: i64, mo: i64, d: i64, h: i64, mi: i64, s: i64) -> Option<i64> {
    let valid = (1..=12).contains(&mo)
        && (1..=31).contains(&d)
        && (0..=23).contains(&h)
        && (0..=59).contains(&mi)
        && (0..=60).contains(&s);
    valid.then(|| days_from_civil(y, mo, d) * 86_400 + h * 3600 + mi * 60 + s)
}

fn num(s: &str) -> Option<i64> {
    (!s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
        .then(|| s.parse().ok())
        .flatten()
}

/// `"2026:07:18 15:38:56"` (EXIF) or `"2026-07-18T15:38:56+0530"` (QuickTime) → unix seconds
/// of the written wall-clock time. Zero dates (`0000:00:00 ...`) and years before 1900 are
/// treated as missing.
pub fn parse_exif_datetime(s: &str) -> Option<i64> {
    let s = s.trim().trim_end_matches('\0');
    let b = s.as_bytes();
    if b.len() < 19 || !matches!(b[4], b':' | b'-') || !matches!(b[10], b' ' | b'T') {
        return None;
    }
    let (y, mo, d) = (num(&s[0..4])?, num(&s[5..7])?, num(&s[8..10])?);
    let (h, mi, sec) = (num(&s[11..13])?, num(&s[14..16])?, num(&s[17..19])?);
    if y < 1900 {
        return None;
    }
    to_secs(y, mo, d, h, mi, sec)
}

/// Unix seconds → `"YYYY-MM-DD HH:MM:SS"`.
pub fn format_local(secs: i64) -> String {
    let (y, m, d) = civil_from_days(secs.div_euclid(86_400));
    let t = secs.rem_euclid(86_400);
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02}",
        t / 3600,
        t % 3600 / 60,
        t % 60
    )
}

/// `2026-11-09 14:03` for a unix timestamp, in the local time zone (UTC when the zone is unknown).
/// The one formatter for run times, so the command line and the app always agree.
pub fn format_run_time(ts: i64) -> String {
    format_run_time_at(
        ts,
        time::UtcOffset::local_offset_at(
            time::OffsetDateTime::from_unix_timestamp(ts)
                .unwrap_or(time::OffsetDateTime::UNIX_EPOCH),
        )
        .ok(),
    )
}

/// Like [`format_run_time`] with a fixed offset (`None` = UTC).
pub fn format_run_time_at(ts: i64, offset: Option<time::UtcOffset>) -> String {
    use time::macros::format_description;
    let fmt = format_description!("[year]-[month]-[day] [hour]:[minute]");
    time::OffsetDateTime::from_unix_timestamp(ts)
        .ok()
        .map(|t| t.to_offset(offset.unwrap_or(time::UtcOffset::UTC)))
        .and_then(|t| t.format(&fmt).ok())
        .unwrap_or_else(|| ts.to_string())
}

/// Seconds since 1904-01-01 (QuickTime epoch) → unix seconds. Unset (0) or pre-1970 → `None`.
pub(crate) fn from_mac_epoch(secs: u64) -> Option<i64> {
    const OFFSET: u64 = 2_082_844_800;
    (secs > OFFSET).then(|| (secs - OFFSET) as i64)
}

#[cfg(test)]
mod tests {
    #[test]
    fn run_time_uses_the_given_offset() {
        let ist = time::UtcOffset::from_hms(5, 30, 0).ok();
        assert_eq!(super::format_run_time_at(0, None), "1970-01-01 00:00");
        assert_eq!(super::format_run_time_at(0, ist), "1970-01-01 05:30");
        assert_eq!(
            super::format_run_time_at(1_800_000_000, None),
            "2027-01-15 08:00"
        );
    }

    use super::*;

    #[test]
    fn parses_exif_and_iso_forms() {
        assert_eq!(parse_exif_datetime("1970:01:01 00:00:00"), Some(0));
        let t = parse_exif_datetime("2026:07:18 15:38:56").unwrap();
        assert_eq!(format_local(t), "2026-07-18 15:38:56");
        assert_eq!(parse_exif_datetime("2026-07-18T15:38:56+0530"), Some(t));
        assert_eq!(
            parse_exif_datetime("2024-02-29 23:59:59"),
            Some(1_709_251_199)
        );
    }

    #[test]
    fn rejects_garbage_and_zero_dates() {
        for s in [
            "",
            "0000:00:00 00:00:00",
            "2026:13:01 00:00:00",
            "not a date at all",
            "2026:07:18",
        ] {
            assert_eq!(parse_exif_datetime(s), None, "{s}");
        }
    }

    #[test]
    fn civil_roundtrip() {
        for d in [-1000, 0, 1, 59, 60, 365, 11_000, 20_000, 40_000] {
            let (y, m, dd) = civil_from_days(d);
            assert_eq!(days_from_civil(y, m, dd), d);
        }
    }

    #[test]
    fn mac_epoch_unset_is_missing() {
        assert_eq!(from_mac_epoch(0), None);
        assert_eq!(from_mac_epoch(2_082_844_800 + 5), Some(5));
    }
}
