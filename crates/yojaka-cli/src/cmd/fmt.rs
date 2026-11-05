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

#[cfg(test)]
mod tests {
    #[test]
    fn formats_sizes() {
        assert_eq!(super::bytes(999), "999 B");
        assert_eq!(super::bytes(1_500), "1.5 KB");
        assert_eq!(super::bytes(2_300_000_000), "2.3 GB");
    }
}
