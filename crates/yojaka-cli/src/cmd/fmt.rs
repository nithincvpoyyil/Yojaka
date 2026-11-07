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

/// `3725000` → `1h 02m 05s`; under a minute → `42s`.
pub fn duration(ms: u64) -> String {
    let s = ms / 1000;
    match (s / 3600, s % 3600 / 60, s % 60) {
        (0, 0, sec) => format!("{sec}s"),
        (0, m, sec) => format!("{m}m {sec:02}s"),
        (h, m, sec) => format!("{h}h {m:02}m {sec:02}s"),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn formats_sizes() {
        assert_eq!(super::bytes(999), "999 B");
        assert_eq!(super::bytes(1_500), "1.5 KB");
        assert_eq!(super::bytes(2_300_000_000), "2.3 GB");
    }

    #[test]
    fn formats_durations() {
        assert_eq!(super::duration(42_000), "42s");
        assert_eq!(super::duration(125_000), "2m 05s");
        assert_eq!(super::duration(3_725_000), "1h 02m 05s");
    }
}
