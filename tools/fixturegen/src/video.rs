//! Synthetic ISO-BMFF / QuickTime videos: valid `ftyp` + `moov` (`mvhd`, `tkhd`, `mdhd`, `hdlr`,
//! `stsd`, `stsz`, and Apple `mdta` keys) around a padded, sparse `mdat`. `moov` can come before
//! or after `mdat`. No ffmpeg needed, and a "4 GB" video costs no disk space.

use std::io::{Seek, SeekFrom, Write};
use std::path::Path;

use crate::{GenError, io};

#[derive(Debug, Clone)]
pub struct VideoSpec {
    /// `qt  ` brand (a .MOV) instead of `isom` (an .MP4).
    pub quicktime: bool,
    /// Put `moov` after `mdat`, as many cameras and phones do.
    pub moov_last: bool,
    /// Size of the (zero-filled, sparse) media payload.
    pub mdat_len: u64,
    pub duration_ms: u64,
    pub width: u32,
    pub height: u32,
    /// 0, 90, 180 or 270.
    pub rotation: u16,
    pub fps: u32,
    pub video_codec: [u8; 4],
    pub audio_codec: Option<[u8; 4]>,
    /// `mvhd` creation time as unix seconds (UTC). `None` writes 0 ("unset").
    pub mvhd_unix: Option<i64>,
    /// `com.apple.quicktime.creationdate`, e.g. `2023-08-12T10:15:30+0530`.
    pub creationdate: Option<String>,
    /// `com.apple.quicktime.location.ISO6709`, e.g. `+15.4989+073.8278+012.000/`.
    pub iso6709: Option<String>,
    /// Android-style `udta/©xyz` location.
    pub xyz: Option<String>,
    pub make: Option<String>,
    pub model: Option<String>,
    /// `com.apple.quicktime.content.identifier` (Live Photo pairing).
    pub content_id: Option<String>,
}

impl Default for VideoSpec {
    fn default() -> Self {
        Self {
            quicktime: true,
            moov_last: false,
            mdat_len: 4096,
            duration_ms: 3000,
            width: 1920,
            height: 1080,
            rotation: 0,
            fps: 30,
            video_codec: *b"hvc1",
            audio_codec: Some(*b"mp4a"),
            mvhd_unix: None,
            creationdate: None,
            iso6709: None,
            xyz: None,
            make: None,
            model: None,
            content_id: None,
        }
    }
}

fn boxed(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut b = ((body.len() + 8) as u32).to_be_bytes().to_vec();
    b.extend(kind);
    b.extend(body);
    b
}

fn full(kind: &[u8; 4], version: u8, flags: u32, body: &[u8]) -> Vec<u8> {
    let mut v = vec![version];
    v.extend(&flags.to_be_bytes()[1..]);
    v.extend(body);
    boxed(kind, &v)
}

fn mac_secs(unix: Option<i64>) -> u32 {
    unix.map_or(0, |u| (u + 2_082_844_800) as u32)
}

fn matrix(rotation: u16) -> [i32; 4] {
    match rotation {
        90 => [0, 0x10000, -0x10000, 0],
        180 => [-0x10000, 0, 0, -0x10000],
        270 => [0, -0x10000, 0x10000, 0],
        _ => [0x10000, 0, 0, 0x10000],
    }
}

fn trak(spec: &VideoSpec, video: bool) -> Vec<u8> {
    let timescale = if video { 600u32 } else { 44_100 };
    let dur = (spec.duration_ms * u64::from(timescale) / 1000) as u32;
    let mut tkhd = Vec::new();
    tkhd.extend([0; 8]); // creation, modification
    tkhd.extend(if video { 1u32 } else { 2 }.to_be_bytes());
    tkhd.extend([0; 4]);
    tkhd.extend((spec.duration_ms as u32).to_be_bytes());
    tkhd.extend([0; 8]); // reserved
    tkhd.extend([0; 4]); // layer, alternate group
    tkhd.extend(if video { [0u8, 0] } else { [1, 0] }); // volume
    tkhd.extend([0; 2]);
    let [a, b, c, d] = if video {
        matrix(spec.rotation)
    } else {
        matrix(0)
    };
    for v in [a, b, 0, c, d, 0, 0, 0, 0x4000_0000] {
        tkhd.extend(v.to_be_bytes());
    }
    let (w, h) = if video {
        (spec.width, spec.height)
    } else {
        (0, 0)
    };
    tkhd.extend((w << 16).to_be_bytes());
    tkhd.extend((h << 16).to_be_bytes());

    let mut mdhd = vec![0; 8];
    mdhd.extend(timescale.to_be_bytes());
    mdhd.extend(dur.to_be_bytes());
    mdhd.extend([0x55, 0xC4, 0, 0]);

    let mut hdlr = vec![0; 4];
    hdlr.extend(if video { b"vide" } else { b"soun" });
    hdlr.extend([0; 12]);
    hdlr.push(0);

    let codec = if video {
        spec.video_codec
    } else {
        spec.audio_codec.unwrap_or(*b"mp4a")
    };
    let mut entry = vec![0; 6];
    entry.extend(1u16.to_be_bytes());
    entry.extend([0; 16]);
    let mut stsd = 1u32.to_be_bytes().to_vec();
    stsd.extend(((entry.len() + 8) as u32).to_be_bytes());
    stsd.extend(codec);
    stsd.extend(&entry);

    let samples = if video {
        (spec.duration_ms * u64::from(spec.fps) / 1000) as u32
    } else {
        (spec.duration_ms * 43 / 1000) as u32
    };
    let mut stsz = 0u32.to_be_bytes().to_vec(); // sample size 0 = per-sample table
    stsz.extend(samples.to_be_bytes());
    stsz.extend(vec![0u8; samples.min(64) as usize * 4]); // a few table entries; the rest is "elided"

    let stbl = boxed(
        b"stbl",
        &[full(b"stsd", 0, 0, &stsd), full(b"stsz", 0, 0, &stsz)].concat(),
    );
    let minf = boxed(b"minf", &stbl);
    let mdia = boxed(
        b"mdia",
        &[full(b"mdhd", 0, 0, &mdhd), full(b"hdlr", 0, 0, &hdlr), minf].concat(),
    );
    boxed(b"trak", &[full(b"tkhd", 0, 3, &tkhd), mdia].concat())
}

fn mdta_meta(spec: &VideoSpec) -> Option<Vec<u8>> {
    let items: Vec<(&str, &String)> = [
        ("com.apple.quicktime.make", &spec.make),
        ("com.apple.quicktime.model", &spec.model),
        ("com.apple.quicktime.creationdate", &spec.creationdate),
        ("com.apple.quicktime.location.ISO6709", &spec.iso6709),
        ("com.apple.quicktime.content.identifier", &spec.content_id),
    ]
    .into_iter()
    .filter_map(|(k, v)| v.as_ref().map(|v| (k, v)))
    .collect();
    if items.is_empty() {
        return None;
    }
    let mut hdlr = vec![0; 4];
    hdlr.extend(b"mdta");
    hdlr.extend([0; 12]);
    hdlr.push(0);
    let mut keys = (items.len() as u32).to_be_bytes().to_vec();
    let mut ilst = Vec::new();
    for (i, (k, v)) in items.iter().enumerate() {
        keys.extend(((k.len() + 8) as u32).to_be_bytes());
        keys.extend(b"mdta");
        keys.extend(k.as_bytes());
        let mut data = 1u32.to_be_bytes().to_vec();
        data.extend([0; 4]);
        data.extend(v.as_bytes());
        ilst.extend(boxed(&(i as u32 + 1).to_be_bytes(), &boxed(b"data", &data)));
    }
    // QuickTime style: a plain box (no version/flags) holding hdlr, keys, ilst.
    Some(boxed(
        b"meta",
        &[
            full(b"hdlr", 0, 0, &hdlr),
            full(b"keys", 0, 0, &keys),
            boxed(b"ilst", &ilst),
        ]
        .concat(),
    ))
}

pub fn moov_bytes(spec: &VideoSpec) -> Vec<u8> {
    let timescale = 1000u32;
    let mut mvhd = Vec::new();
    mvhd.extend(mac_secs(spec.mvhd_unix).to_be_bytes());
    mvhd.extend(mac_secs(spec.mvhd_unix).to_be_bytes());
    mvhd.extend(timescale.to_be_bytes());
    mvhd.extend((spec.duration_ms as u32).to_be_bytes());
    mvhd.extend(0x0001_0000u32.to_be_bytes()); // rate
    mvhd.extend([1, 0, 0, 0]); // volume + reserved
    mvhd.extend([0; 8]);
    for v in [0x10000i32, 0, 0, 0, 0x10000, 0, 0, 0, 0x4000_0000] {
        mvhd.extend(v.to_be_bytes());
    }
    mvhd.extend([0; 24]);
    mvhd.extend(3u32.to_be_bytes());

    let mut parts = vec![full(b"mvhd", 0, 0, &mvhd), trak(spec, true)];
    if spec.audio_codec.is_some() {
        parts.push(trak(spec, false));
    }
    if let Some(m) = mdta_meta(spec) {
        parts.push(m);
    }
    if let Some(xyz) = &spec.xyz {
        let mut payload = (xyz.len() as u16).to_be_bytes().to_vec();
        payload.extend([0x15, 0xC7]);
        payload.extend(xyz.as_bytes());
        parts.push(boxed(b"udta", &boxed(b"\xA9xyz", &payload)));
    }
    boxed(b"moov", &parts.concat())
}

fn ftyp(spec: &VideoSpec) -> Vec<u8> {
    let (major, compat): (&[u8; 4], &[u8; 4]) = if spec.quicktime {
        (b"qt  ", b"qt  ")
    } else {
        (b"isom", b"mp42")
    };
    let mut b = major.to_vec();
    b.extend(0u32.to_be_bytes());
    b.extend(compat);
    boxed(b"ftyp", &b)
}

/// Writes the video to `path`. The `mdat` payload is a hole in the file (sparse), so even a
/// 4 GB video is created instantly and occupies almost no disk.
pub fn write_video(path: &Path, spec: &VideoSpec) -> Result<(), GenError> {
    let mut f = io(path, std::fs::File::create(path))?;
    let moov = moov_bytes(spec);
    let mut w = |bytes: &[u8]| io(path, f.write_all(bytes));
    w(&ftyp(spec))?;
    if !spec.moov_last {
        w(&moov)?;
    }
    // mdat header: 64-bit "largesize" form when the payload does not fit in 32 bits.
    let total = spec.mdat_len + 8;
    if total > u64::from(u32::MAX) {
        w(&1u32.to_be_bytes())?;
        w(b"mdat")?;
        w(&(spec.mdat_len + 16).to_be_bytes())?;
    } else {
        w(&(total as u32).to_be_bytes())?;
        w(b"mdat")?;
    }
    let pos = io(path, f.stream_position())?;
    io(path, f.seek(SeekFrom::Start(pos + spec.mdat_len)))?;
    io(path, f.set_len(pos + spec.mdat_len))?;
    if spec.moov_last {
        io(path, f.write_all(&moov))?;
    }
    Ok(())
}
