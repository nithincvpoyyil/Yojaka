#![allow(clippy::unwrap_used)]

use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use fixturegen::video::{VideoSpec, write_video};

use super::*;
use crate::model::Format;

/// Counts the bytes actually read from the underlying source.
struct Counting<R> {
    inner: R,
    read: Arc<AtomicU64>,
}

impl<R: Read> Read for Counting<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.read.fetch_add(n as u64, Ordering::Relaxed);
        Ok(n)
    }
}
impl<R: Seek> Seek for Counting<R> {
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        self.inner.seek(pos)
    }
}

fn extract_counted(path: &std::path::Path, format: Format) -> (Result<Meta, MetaError>, u64) {
    let size = std::fs::metadata(path).unwrap().len();
    let read = Arc::new(AtomicU64::new(0));
    let mut r = Counting {
        inner: std::fs::File::open(path).unwrap(),
        read: read.clone(),
    };
    let res = extract(&mut r, &FileInfo { size, format });
    (res, read.load(Ordering::Relaxed))
}

fn tmp() -> tempfile::TempDir {
    tempfile::tempdir().unwrap()
}

fn iphone() -> VideoSpec {
    VideoSpec {
        duration_ms: 12_500,
        rotation: 90,
        creationdate: Some("2023-08-12T10:15:30+0530".into()),
        iso6709: Some("+15.4989+073.8278+012.000/".into()),
        make: Some("Apple".into()),
        model: Some("iPhone 15".into()),
        content_id: Some("ABCD-1234".into()),
        mvhd_unix: Some(1_691_835_330),
        ..VideoSpec::default()
    }
}

#[test]
fn iphone_hevc_mov() {
    let d = tmp();
    let p = d.path().join("a.mov");
    write_video(&p, &iphone()).unwrap();
    let (m, _) = extract_counted(&p, Format::Mov);
    let m = m.unwrap();
    assert_eq!(m.duration_ms, Some(12_500));
    assert_eq!(
        (m.width, m.height, m.rotation),
        (Some(1920), Some(1080), Some(90))
    );
    assert_eq!(m.fps, Some(30.0));
    assert_eq!(m.video_codec.as_deref(), Some("hevc"));
    assert_eq!(m.audio_codec.as_deref(), Some("aac"));
    assert_eq!(m.camera_make.as_deref(), Some("Apple"));
    assert_eq!(m.camera_model.as_deref(), Some("iPhone 15"));
    assert_eq!(m.content_id.as_deref(), Some("ABCD-1234"));
    // The QuickTime creation date (local wall clock) wins over the UTC mvhd time.
    assert_eq!(m.taken_at_src, DateSrc::QuickTime);
    assert_eq!(format_local(m.taken_at.unwrap()), "2023-08-12 10:15:30");
    let g = m.gps.unwrap();
    assert!((g.lat - 15.4989).abs() < 1e-9 && (g.lon - 73.8278).abs() < 1e-9);
}

#[test]
fn android_mp4_uses_udta_location_and_mvhd_date() {
    let d = tmp();
    let p = d.path().join("a.mp4");
    let spec = VideoSpec {
        quicktime: false,
        video_codec: *b"avc1",
        fps: 25,
        duration_ms: 8_000,
        xyz: Some("+37.4219-122.0840/".into()),
        mvhd_unix: Some(1_700_000_000),
        ..VideoSpec::default()
    };
    write_video(&p, &spec).unwrap();
    let m = extract_counted(&p, Format::Mp4).0.unwrap();
    assert_eq!(m.video_codec.as_deref(), Some("h264"));
    assert_eq!(m.fps, Some(25.0));
    assert_eq!(m.taken_at, Some(1_700_000_000));
    assert_eq!(m.taken_at_src, DateSrc::Container);
    let g = m.gps.unwrap();
    assert!((g.lat - 37.4219).abs() < 1e-9 && (g.lon + 122.0840).abs() < 1e-9);
    assert_eq!(m.camera_make, None);
}

#[test]
fn unset_mvhd_date_is_missing_not_1904() {
    let d = tmp();
    let p = d.path().join("a.mov");
    write_video(&p, &VideoSpec::default()).unwrap();
    let m = extract_counted(&p, Format::Mov).0.unwrap();
    assert_eq!(m.taken_at, None);
    assert_eq!(m.taken_at_src, DateSrc::None);
    assert_eq!(m.gps, None);
}

#[test]
fn moov_after_mdat_works() {
    let d = tmp();
    let p = d.path().join("late.mp4");
    let spec = VideoSpec {
        quicktime: false,
        moov_last: true,
        mdat_len: 3 << 20,
        ..iphone()
    };
    write_video(&p, &spec).unwrap();
    let (m, read) = extract_counted(&p, Format::Mp4);
    assert_eq!(m.unwrap().duration_ms, Some(12_500));
    assert!(read < 64 << 10, "read {read} bytes of a 3 MiB file");
}

#[test]
fn four_gigabyte_video_reads_less_than_one_megabyte() {
    let d = tmp();
    for moov_last in [false, true] {
        let p = d.path().join(format!("big{moov_last}.mov"));
        let spec = VideoSpec {
            moov_last,
            mdat_len: 4 << 30,
            ..iphone()
        };
        write_video(&p, &spec).unwrap();
        assert!(std::fs::metadata(&p).unwrap().len() > 4 << 30);
        let (m, read) = extract_counted(&p, Format::Mov);
        assert_eq!(
            m.unwrap().duration_ms,
            Some(12_500),
            "moov_last={moov_last}"
        );
        assert!(read < 1 << 20, "moov_last={moov_last}: read {read} bytes");
    }
}

#[test]
fn broken_videos_are_errors_not_panics() {
    let d = tmp();
    let p = d.path().join("good.mov");
    write_video(&p, &iphone()).unwrap();
    let good = std::fs::read(&p).unwrap();
    // Every truncation point must give an error or valid metadata, never a panic.
    for cut in (0..good.len()).step_by(7) {
        let q = d.path().join("cut.mov");
        std::fs::write(&q, &good[..cut]).unwrap();
        let _ = extract_counted(&q, Format::Mov).0;
    }
    // No moov at all
    let q = d.path().join("nomoov.mp4");
    std::fs::write(&q, &good[..28]).unwrap();
    assert!(extract_counted(&q, Format::Mp4).0.is_err());
    // Not a video
    std::fs::write(&q, b"this is not an mp4 file at all, sorry").unwrap();
    assert!(matches!(
        extract_counted(&q, Format::Mp4).0,
        Err(MetaError::Malformed(_))
    ));
}

#[test]
fn zero_sized_box_cannot_loop_forever() {
    let d = tmp();
    let q = d.path().join("evil.mp4");
    // ftyp, then boxes of size 1..7 (smaller than a header)
    let mut b = vec![
        0, 0, 0, 0x14, b'f', b't', b'y', b'p', b'i', b's', b'o', b'm', 0, 0, 0, 0, b'i', b's',
        b'o', b'm',
    ];
    b.extend([0, 0, 0, 4, b'm', b'o', b'o', b'v']);
    std::fs::write(&q, &b).unwrap();
    assert!(extract_counted(&q, Format::Mp4).0.is_err());
}

// ---- Matroska ---------------------------------------------------------------------------

fn ebml(id: &[u8], body: &[u8]) -> Vec<u8> {
    let mut v = id.to_vec();
    assert!(body.len() < 0x3FFF);
    v.extend([0x40 | (body.len() >> 8) as u8, body.len() as u8]);
    v.extend(body);
    v
}

fn mkv() -> Vec<u8> {
    let info = [
        ebml(&[0x2A, 0xD7, 0xB1], &1_000_000u32.to_be_bytes()),
        ebml(&[0x44, 0x89], &2500.0f64.to_be_bytes()),
    ]
    .concat();
    let video = ebml(
        &[0xE0],
        &[
            ebml(&[0xB0], &1280u16.to_be_bytes()),
            ebml(&[0xBA], &720u16.to_be_bytes()),
        ]
        .concat(),
    );
    let vtrack = ebml(
        &[0xAE],
        &[
            ebml(&[0x83], &[1]),
            ebml(&[0x86], b"V_MPEGH/ISO/HEVC"),
            ebml(&[0x23, 0xE3, 0x83], &40_000_000u32.to_be_bytes()),
            video,
        ]
        .concat(),
    );
    let atrack = ebml(
        &[0xAE],
        &[ebml(&[0x83], &[2]), ebml(&[0x86], b"A_OPUS")].concat(),
    );
    let seg = [
        ebml(&[0x15, 0x49, 0xA9, 0x66], &info),
        ebml(&[0x16, 0x54, 0xAE, 0x6B], &[vtrack, atrack].concat()),
        ebml(&[0x1F, 0x43, 0xB6, 0x75], &[0u8; 100]),
    ]
    .concat();
    [
        ebml(&[0x1A, 0x45, 0xDF, 0xA3], &ebml(&[0x42, 0x82], b"matroska")),
        ebml(&[0x18, 0x53, 0x80, 0x67], &seg),
    ]
    .concat()
}

#[test]
fn matroska_duration_resolution_codecs() {
    let d = tmp();
    let p = d.path().join("a.mkv");
    std::fs::write(&p, mkv()).unwrap();
    let m = extract_counted(&p, Format::Mkv).0.unwrap();
    assert_eq!(m.duration_ms, Some(2500));
    assert_eq!((m.width, m.height), (Some(1280), Some(720)));
    assert_eq!(m.fps, Some(25.0));
    assert_eq!(m.video_codec.as_deref(), Some("hevc"));
    assert_eq!(m.audio_codec.as_deref(), Some("opus"));
}

#[test]
fn truncated_matroska_never_panics() {
    let good = mkv();
    let d = tmp();
    let p = d.path().join("cut.mkv");
    for cut in 0..good.len() {
        std::fs::write(&p, &good[..cut]).unwrap();
        let _ = extract_counted(&p, Format::Mkv).0;
    }
}

// ---- real photos (not in git: tests/data is local) ------------------------------------

fn sample_dir() -> Option<PathBuf> {
    let d = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/data");
    std::fs::read_dir(&d).ok()?;
    Some(d)
}

#[test]
fn real_sony_jpegs_if_present() {
    let Some(dir) = sample_dir() else { return };
    let mut checked = 0;
    for sub in std::fs::read_dir(&dir).unwrap().flatten() {
        if !sub.path().is_dir() {
            continue;
        }
        for f in std::fs::read_dir(sub.path()).unwrap().flatten() {
            let p = f.path();
            if p.extension().and_then(|e| e.to_str()) != Some("JPG") {
                continue;
            }
            let (m, read) = extract_counted(&p, Format::Jpeg);
            let m = m.unwrap_or_else(|e| panic!("{}: {e}", p.display()));
            assert_eq!(m.camera_make.as_deref(), Some("SONY"));
            assert_eq!(m.camera_model.as_deref(), Some("ILCE-6000"));
            assert_eq!(
                (m.width, m.height),
                (Some(4240), Some(2832)),
                "{}",
                p.display()
            );
            assert_eq!(m.taken_at_src, DateSrc::Exif);
            assert!(m.gps.is_none());
            assert!(
                read <= 1 << 20,
                "{}: read {read} bytes of a ~2.5 MB photo",
                p.display()
            );
            checked += 1;
        }
    }
    eprintln!("checked {checked} real photos");
}
