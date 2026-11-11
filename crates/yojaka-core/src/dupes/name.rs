//! Name tier: file names that differ only by what copying adds (`IMG_1234 (1).jpg`,
//! `IMG_1234 copy.jpg`, `IMG_1234-1.jpg`).

use unicode_normalization::UnicodeNormalization;

/// The name with Unicode normalized (NFC), lower-cased, and the extension and any copy suffixes
/// removed. Two files are name-tier candidates when this is equal.
pub fn normalize(name: &[u8]) -> String {
    let text = String::from_utf8_lossy(name);
    let text: String = text.nfc().collect::<String>().to_lowercase();
    let stem = match text.rfind('.') {
        Some(i) if i > 0 => &text[..i],
        _ => text.as_str(),
    };
    let mut stem = stem.trim();
    loop {
        let next = strip_suffix(stem).trim_end();
        if next.is_empty() || next == stem {
            break;
        }
        stem = next;
    }
    stem.to_owned()
}

/// Removes one copy suffix, or returns `s` unchanged.
fn strip_suffix(s: &str) -> &str {
    // "name (2)" and "name(2)"
    if let Some(body) = s.strip_suffix(')')
        && let Some(open) = body.rfind('(')
        && is_number(&body[open + 1..])
    {
        return &s[..open];
    }
    // "name copy", "name copy 3", "name - copy"
    let (head, tail) = split_trailing_number(s);
    for word in [" copy", "-copy", "_copy"] {
        if let Some(rest) = head.trim_end().strip_suffix(word) {
            return rest.trim_end_matches([' ', '-']);
        }
    }
    // "name-1" and "name_1": one digit only, so the counter in `IMG_1234` is left alone.
    if tail.len() == 1
        && tail != "0"
        && let Some(rest) = head.strip_suffix(['-', '_'])
    {
        return rest;
    }
    s
}

fn is_number(s: &str) -> bool {
    !s.is_empty() && s.len() <= 3 && s.bytes().all(|b| b.is_ascii_digit())
}

/// Splits `abc 12` into (`abc `, `12`); the second part is empty when `s` does not end in digits.
fn split_trailing_number(s: &str) -> (&str, &str) {
    let digits = s.bytes().rev().take_while(u8::is_ascii_digit).count();
    s.split_at(s.len() - digits)
}

#[cfg(test)]
mod tests {
    use super::normalize;

    fn n(s: &str) -> String {
        normalize(s.as_bytes())
    }

    #[test]
    fn copy_suffixes_are_removed() {
        for name in [
            "IMG_1234.jpg",
            "IMG_1234 (1).jpg",
            "IMG_1234(2).JPG",
            "IMG_1234 copy.jpg",
            "IMG_1234 copy 2.jpg",
            "IMG_1234 - Copy.jpg",
            "IMG_1234-1.jpg",
            "IMG_1234_1.jpeg",
            "img_1234 (1) copy.png",
        ] {
            assert_eq!(n(name), "img_1234", "{name}");
        }
    }

    #[test]
    fn real_counters_and_words_are_kept() {
        assert_eq!(n("IMG_1234.jpg"), "img_1234");
        assert_ne!(n("IMG_1234.jpg"), n("IMG_1235.jpg"));
        assert_eq!(n("IMG_0010.jpg"), "img_0010");
        assert_eq!(n("photocopy.jpg"), "photocopy");
        assert_eq!(n("2020-10-12.jpg"), "2020-10-12");
        assert_eq!(n("scan_10.png"), "scan_10");
        assert_eq!(n("party (2019).jpg"), "party (2019)");
    }

    #[test]
    fn a_name_is_never_emptied() {
        assert_eq!(n("(1).jpg"), "(1)");
        assert_eq!(n("copy.jpg"), "copy");
        assert_eq!(n(".hidden"), ".hidden");
        assert_eq!(n("-1.jpg"), "-1");
    }

    #[test]
    fn unicode_forms_compare_equal() {
        // "é" precomposed and as "e" + combining accent.
        assert_eq!(n("caf\u{e9}.jpg"), n("cafe\u{301}.jpg"));
    }
}
