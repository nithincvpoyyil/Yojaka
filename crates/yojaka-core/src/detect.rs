//! File type detection: extension plus magic bytes.
//!
//! - A known extension is confirmed (or corrected) by the first bytes of the file.
//! - Unknown or missing extensions are only sniffed with `sniff_all`.
//! - **Cloud-only (dataless) files are never opened**: the extension alone decides.

use std::io::Read;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

use crate::model::Format;

/// Bytes read from the start of a file. Never more.
pub const SNIFF_LEN: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Sniffed {
    Definite(Format),
    Tiff,
    Ebml,
    IsoVideo(Format),
}

fn brand_format(brand: &[u8]) -> Option<Sniffed> {
    Some(match brand {
        b"heic" | b"heix" | b"hevc" | b"hevx" | b"heim" | b"heis" | b"mif1" | b"msf1" => {
            Sniffed::Definite(Format::Heic)
        }
        b"avif" | b"avis" => Sniffed::Definite(Format::Avif),
        b"crx " => Sniffed::Definite(Format::RawCr3),
        b"qt  " => Sniffed::IsoVideo(Format::Mov),
        b"M4V " | b"M4VH" | b"M4VP" => Sniffed::IsoVideo(Format::M4v),
        b if b.starts_with(b"3g") => Sniffed::IsoVideo(Format::ThreeGp),
        _ => Sniffed::IsoVideo(Format::Mp4),
    })
}

fn sniff(head: &[u8]) -> Option<Sniffed> {
    use Sniffed::*;
    if head.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some(Definite(Format::Jpeg));
    }
    if head.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        return Some(Definite(Format::Png));
    }
    if head.starts_with(b"GIF87a") || head.starts_with(b"GIF89a") {
        return Some(Definite(Format::Gif));
    }
    if head.len() >= 12 && &head[..4] == b"RIFF" {
        match &head[8..12] {
            b"WEBP" => return Some(Definite(Format::Webp)),
            b"AVI " => return Some(Definite(Format::Avi)),
            _ => {}
        }
    }
    if head.len() >= 12 && &head[4..8] == b"ftyp" {
        return brand_format(&head[8..12]);
    }
    if head.starts_with(b"II*\0") || head.starts_with(b"MM\0*") {
        if head.len() >= 10 && &head[8..10] == b"CR" {
            return Some(Definite(Format::RawCr2));
        }
        return Some(Tiff);
    }
    if head.starts_with(b"IIRO") || head.starts_with(b"IIRS") || head.starts_with(b"MMOR") {
        return Some(Definite(Format::RawOrf));
    }
    if head.starts_with(b"FUJIFILMCCD-RAW") {
        return Some(Definite(Format::RawRaf));
    }
    if head.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]) {
        return Some(Ebml);
    }
    // BMP: "BM" alone is too weak; also require a sane reserved field and DIB header size.
    if head.len() >= 18 && head.starts_with(b"BM") && head[6..10] == [0, 0, 0, 0] {
        let dib = u32::from_le_bytes([head[14], head[15], head[16], head[17]]);
        if matches!(dib, 12 | 40 | 52 | 56 | 64 | 108 | 124) {
            return Some(Definite(Format::Bmp));
        }
    }
    None
}

fn looks_like_mpeg_ts(head: &[u8]) -> bool {
    head.first() == Some(&0x47) || head.get(4) == Some(&0x47)
}

/// Combines the extension guess with what the bytes say. Magic wins for renamed files.
fn resolve(ext_fmt: Option<Format>, head: &[u8]) -> Option<Format> {
    match sniff(head) {
        Some(Sniffed::Definite(f)) => Some(f),
        Some(Sniffed::Tiff) => match ext_fmt {
            Some(
                f @ (Format::RawDng
                | Format::RawCr2
                | Format::RawNef
                | Format::RawArw
                | Format::Tiff),
            ) => Some(f),
            _ => Some(Format::Tiff),
        },
        Some(Sniffed::Ebml) => Some(if ext_fmt == Some(Format::Webm) {
            Format::Webm
        } else {
            Format::Mkv
        }),
        Some(Sniffed::IsoVideo(brand_fmt)) => match ext_fmt {
            Some(f @ (Format::Mov | Format::M4v | Format::ThreeGp | Format::Mp4)) => Some(f),
            _ => Some(brand_fmt),
        },
        None => (ext_fmt == Some(Format::Mts) && looks_like_mpeg_ts(head)).then_some(Format::Mts),
    }
}

fn extension(path: &Path) -> Option<&str> {
    path.extension().and_then(|e| e.to_str())
}

/// Detection with an injectable reader of the file head (tests use it to prove that
/// dataless files are never opened).
pub fn detect_with(
    path: &Path,
    dataless: bool,
    sniff_all: bool,
    read_head: &dyn Fn(&Path) -> std::io::Result<Vec<u8>>,
) -> std::io::Result<Option<Format>> {
    let ext_fmt = extension(path).and_then(Format::from_extension);
    if dataless {
        return Ok(ext_fmt);
    }
    if ext_fmt.is_none() && !sniff_all {
        return Ok(None);
    }
    let head = read_head(path)?;
    Ok(resolve(ext_fmt, &head))
}

fn read_head_real(path: &Path) -> std::io::Result<Vec<u8>> {
    // O_NOFOLLOW: if the path became a symlink after it was checked, the open fails
    // instead of reading through the link.
    let mut f = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)?;
    let mut buf = vec![0u8; SNIFF_LEN];
    let mut n = 0;
    while n < SNIFF_LEN {
        match f.read(&mut buf[n..])? {
            0 => break,
            k => n += k,
        }
    }
    buf.truncate(n);
    Ok(buf)
}

/// Detects the format of `path`, reading at most [`SNIFF_LEN`] bytes.
pub fn detect(path: &Path, dataless: bool, sniff_all: bool) -> std::io::Result<Option<Format>> {
    detect_with(path, dataless, sniff_all, &read_head_real)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn head(bytes: &[u8]) -> Vec<u8> {
        bytes.to_vec()
    }

    fn ftyp(brand: &[u8; 4]) -> Vec<u8> {
        let mut v = vec![0, 0, 0, 0x18, b'f', b't', b'y', b'p'];
        v.extend(brand);
        v.extend([0; 16]);
        v
    }

    #[test]
    fn ftyp_brand_separates_heic_from_mov_and_mp4() {
        assert_eq!(
            resolve(Some(Format::Heic), &ftyp(b"heic")),
            Some(Format::Heic)
        );
        assert_eq!(
            resolve(Some(Format::Mov), &ftyp(b"qt  ")),
            Some(Format::Mov)
        );
        assert_eq!(
            resolve(Some(Format::Mp4), &ftyp(b"isom")),
            Some(Format::Mp4)
        );
        assert_eq!(
            resolve(Some(Format::Mov), &ftyp(b"isom")),
            Some(Format::Mov)
        );
        assert_eq!(
            resolve(Some(Format::Mp4), &ftyp(b"3gp5")),
            Some(Format::Mp4)
        );
        assert_eq!(resolve(None, &ftyp(b"3gp5")), Some(Format::ThreeGp));
        assert_eq!(
            resolve(Some(Format::Heic), &ftyp(b"qt  ")),
            Some(Format::Mov)
        );
        assert_eq!(
            resolve(Some(Format::Mov), &ftyp(b"avif")),
            Some(Format::Avif)
        );
    }

    #[test]
    fn renamed_files_are_found_by_magic() {
        assert_eq!(
            resolve(Some(Format::Png), &[0xFF, 0xD8, 0xFF, 0xE0]),
            Some(Format::Jpeg)
        );
        assert_eq!(resolve(None, b"GIF89a\0\0"), Some(Format::Gif));
    }

    #[test]
    fn fake_media_is_rejected() {
        assert_eq!(resolve(Some(Format::Jpeg), b"hello, I am text"), None);
        assert_eq!(resolve(Some(Format::Jpeg), b""), None);
        assert_eq!(resolve(None, b"BM is not enough"), None);
    }

    #[test]
    fn tiff_family_uses_extension_for_raw() {
        let tiff = head(b"II*\0\x08\0\0\0\0\0\0\0");
        assert_eq!(resolve(Some(Format::RawDng), &tiff), Some(Format::RawDng));
        assert_eq!(resolve(Some(Format::RawNef), &tiff), Some(Format::RawNef));
        assert_eq!(resolve(Some(Format::Tiff), &tiff), Some(Format::Tiff));
        assert_eq!(resolve(Some(Format::Jpeg), &tiff), Some(Format::Tiff));
        let cr2 = head(b"II*\0\x10\0\0\0CR\x02\0");
        assert_eq!(resolve(Some(Format::Tiff), &cr2), Some(Format::RawCr2));
    }

    #[test]
    fn other_containers() {
        let ebml = [0x1A, 0x45, 0xDF, 0xA3, 0, 0];
        assert_eq!(resolve(Some(Format::Webm), &ebml), Some(Format::Webm));
        assert_eq!(resolve(Some(Format::Mkv), &ebml), Some(Format::Mkv));
        let mut avi = b"RIFF\0\0\0\0AVI LIST".to_vec();
        avi.extend([0; 8]);
        assert_eq!(resolve(Some(Format::Avi), &avi), Some(Format::Avi));
        assert_eq!(
            resolve(Some(Format::Mts), &[0x47, 0x40, 0x11]),
            Some(Format::Mts)
        );
        assert_eq!(resolve(Some(Format::Mts), b"text"), None);
        let mut bmp = vec![b'B', b'M', 0, 0, 0, 0, 0, 0, 0, 0, 54, 0, 0, 0];
        bmp.extend(40u32.to_le_bytes());
        assert_eq!(resolve(None, &bmp), Some(Format::Bmp));
    }

    #[test]
    fn dataless_files_are_never_opened() {
        let boom = |_: &Path| -> std::io::Result<Vec<u8>> { panic!("dataless file was opened") };
        let got = detect_with(Path::new("/nonexistent/IMG_1.HEIC"), true, false, &boom).unwrap();
        assert_eq!(got, Some(Format::Heic));
        let got = detect_with(Path::new("/nonexistent/readme"), true, true, &boom).unwrap();
        assert_eq!(got, None);
    }

    #[test]
    fn unknown_extension_is_skipped_without_io_unless_sniff_all() {
        let boom = |_: &Path| -> std::io::Result<Vec<u8>> { panic!("opened") };
        assert_eq!(
            detect_with(Path::new("/x/notes.txt"), false, false, &boom).unwrap(),
            None
        );
        let jpeg = |_: &Path| Ok(vec![0xFF, 0xD8, 0xFF, 0xE0]);
        assert_eq!(
            detect_with(Path::new("/x/noext"), false, true, &jpeg).unwrap(),
            Some(Format::Jpeg)
        );
    }

    #[test]
    fn symlinks_are_never_opened() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("real.jpg");
        std::fs::write(&target, [0xFF, 0xD8, 0xFF, 0xE0]).unwrap();
        let link = tmp.path().join("link.jpg");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert!(read_head_real(&target).is_ok());
        assert!(read_head_real(&link).is_err());
        assert!(detect(&link, false, false).is_err());
    }

    #[test]
    fn real_file_is_read_at_most_sniff_len() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("a.jpg");
        let mut data = vec![0xFF, 0xD8, 0xFF, 0xE0];
        data.resize(10_000, 0);
        std::fs::write(&p, data).unwrap();
        assert_eq!(read_head_real(&p).unwrap().len(), SNIFF_LEN);
        assert_eq!(detect(&p, false, false).unwrap(), Some(Format::Jpeg));
    }
}
