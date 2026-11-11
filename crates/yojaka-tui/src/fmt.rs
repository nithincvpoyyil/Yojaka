//! Small text helpers shared by the screens.

/// `1234567` → `1,234,567`.
pub fn thousands(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// `1 file`, `1,200 files`.
pub fn files(n: u64) -> String {
    if n == 1 {
        "1 file".into()
    } else {
        format!("{} files", thousands(n))
    }
}

/// Decimal units, like the CLI: `1_500` → `1.5 KB`.
pub fn bytes(n: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = n as f64;
    let mut i = 0;
    while v >= 1000.0 && i < UNITS.len() - 1 {
        v /= 1000.0;
        i += 1;
    }
    if i == 0 {
        format!("{n} B")
    } else {
        format!("{v:.1} {}", UNITS[i])
    }
}

/// `3725000` → `1:02:05`; `125000` → `2:05`. For table cells.
pub fn clock(ms: u64) -> String {
    let s = ms / 1000;
    match (s / 3600, s % 3600 / 60, s % 60) {
        (0, m, sec) => format!("{m}:{sec:02}"),
        (h, m, sec) => format!("{h}:{m:02}:{sec:02}"),
    }
}

/// `3725000` → `1h 02m`; `42000` → `42s`. For totals.
pub fn span(ms: u64) -> String {
    let s = ms / 1000;
    match (s / 86_400, s % 86_400 / 3600, s % 3600 / 60, s % 60) {
        (0, 0, 0, sec) => format!("{sec}s"),
        (0, 0, m, sec) => format!("{m}m {sec:02}s"),
        (0, h, m, _) => format!("{h}h {m:02}m"),
        (d, h, _, _) => format!("{d}d {h}h"),
    }
}

/// Cuts `s` to `width` characters, ending in `…` when it was cut.
pub fn truncate(s: &str, width: usize) -> String {
    if s.chars().count() <= width {
        return s.to_string();
    }
    if width == 0 {
        return String::new();
    }
    let mut out: String = s.chars().take(width - 1).collect();
    out.push('…');
    out
}

/// Like [`truncate`] but keeps the end (the interesting part of a path).
pub fn truncate_start(s: &str, width: usize) -> String {
    let n = s.chars().count();
    if n <= width {
        return s.to_string();
    }
    if width == 0 {
        return String::new();
    }
    let mut out = String::from("…");
    out.extend(s.chars().skip(n - (width - 1)));
    out
}

/// A horizontal bar `width` cells wide for `value / max`, in eighth-block steps.
pub fn bar(value: u64, max: u64, width: usize) -> String {
    const PARTIAL: [char; 8] = [' ', '▏', '▎', '▍', '▌', '▋', '▊', '▉'];
    if max == 0 || width == 0 {
        return String::new();
    }
    let eighths = (value as u128 * width as u128 * 8 / max as u128) as usize;
    let mut s = "█".repeat(eighths / 8);
    // A non-zero value always shows at least a sliver.
    let rest = eighths % 8;
    if rest > 0 {
        s.push(PARTIAL[rest]);
    } else if value > 0 && s.is_empty() {
        s.push(PARTIAL[1]);
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thousands_separators() {
        assert_eq!(thousands(0), "0");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(1_000), "1,000");
        assert_eq!(thousands(1_234_567), "1,234,567");
    }

    #[test]
    fn sizes_and_durations() {
        assert_eq!(bytes(999), "999 B");
        assert_eq!(bytes(2_300_000_000), "2.3 GB");
        assert_eq!(clock(125_000), "2:05");
        assert_eq!(clock(3_725_000), "1:02:05");
        assert_eq!(span(42_000), "42s");
        assert_eq!(span(3_725_000), "1h 02m");
        assert_eq!(span(200_000_000), "2d 7h");
    }

    #[test]
    fn truncation_counts_characters() {
        assert_eq!(truncate("abcdef", 4), "abc…");
        assert_eq!(truncate("abc", 4), "abc");
        assert_eq!(truncate("héllo wörld", 5), "héll…");
        assert_eq!(truncate_start("/a/b/c/d.jpg", 7), "…/d.jpg");
        assert_eq!(truncate("abc", 0), "");
    }

    #[test]
    fn bars_scale_and_never_vanish() {
        assert_eq!(bar(10, 10, 4), "████");
        assert_eq!(bar(5, 10, 4), "██");
        assert_eq!(bar(1, 1000, 4), "▏");
        assert_eq!(bar(0, 10, 4), "");
        assert_eq!(bar(3, 0, 4), "");
    }
}
