//! A minimal little-endian TIFF/EXIF writer: just enough for DateTimeOriginal, Make, Model,
//! Orientation and a GPS IFD, spliced into a JPEG as an APP1 segment.

pub struct ExifData<'a> {
    pub make: &'a str,
    pub model: &'a str,
    pub orientation: u16,
    /// `YYYY:MM:DD HH:MM:SS`
    pub date_time_original: &'a str,
    /// (latitude, longitude) in signed decimal degrees.
    pub gps: Option<(f64, f64)>,
}

struct Entry {
    tag: u16,
    typ: u16,
    count: u32,
    data: Vec<u8>,
}

fn ascii(tag: u16, s: &str) -> Entry {
    let mut data = s.as_bytes().to_vec();
    data.push(0);
    Entry {
        tag,
        typ: 2,
        count: data.len() as u32,
        data,
    }
}
fn short(tag: u16, v: u16) -> Entry {
    Entry {
        tag,
        typ: 3,
        count: 1,
        data: v.to_le_bytes().to_vec(),
    }
}
fn long(tag: u16, v: u32) -> Entry {
    Entry {
        tag,
        typ: 4,
        count: 1,
        data: v.to_le_bytes().to_vec(),
    }
}
fn rationals(tag: u16, vals: &[(u32, u32)]) -> Entry {
    let mut data = Vec::new();
    for (n, d) in vals {
        data.extend(n.to_le_bytes());
        data.extend(d.to_le_bytes());
    }
    Entry {
        tag,
        typ: 5,
        count: vals.len() as u32,
        data,
    }
}

fn pad2(n: usize) -> usize {
    n + (n & 1)
}

fn ifd_size(entries: &[Entry]) -> usize {
    2 + 12 * entries.len()
        + 4
        + entries
            .iter()
            .filter(|e| e.data.len() > 4)
            .map(|e| pad2(e.data.len()))
            .sum::<usize>()
}

/// Appends one IFD at the current end of `out` (offsets are relative to the TIFF start).
fn write_ifd(out: &mut Vec<u8>, mut entries: Vec<Entry>) {
    entries.sort_by_key(|e| e.tag);
    let mut data_off = out.len() + 2 + 12 * entries.len() + 4;
    out.extend((entries.len() as u16).to_le_bytes());
    for e in &entries {
        out.extend(e.tag.to_le_bytes());
        out.extend(e.typ.to_le_bytes());
        out.extend(e.count.to_le_bytes());
        if e.data.len() <= 4 {
            let mut v = e.data.clone();
            v.resize(4, 0);
            out.extend(v);
        } else {
            out.extend((data_off as u32).to_le_bytes());
            data_off += pad2(e.data.len());
        }
    }
    out.extend(0u32.to_le_bytes());
    for e in entries.iter().filter(|e| e.data.len() > 4) {
        out.extend(&e.data);
        if e.data.len() & 1 == 1 {
            out.push(0);
        }
    }
}

fn dms(v: f64) -> [(u32, u32); 3] {
    let v = v.abs();
    let deg = v.floor();
    let min_f = (v - deg) * 60.0;
    let min = min_f.floor();
    let sec = ((min_f - min) * 60.0 * 10_000.0).round() as u32;
    [(deg as u32, 1), (min as u32, 1), (sec, 10_000)]
}

/// Builds the TIFF block (without the `Exif\0\0` prefix).
pub fn build_tiff(d: &ExifData) -> Vec<u8> {
    let ifd0 = |exif_ptr: u32, gps_ptr: Option<u32>| {
        let mut v = vec![
            ascii(0x010F, d.make),
            ascii(0x0110, d.model),
            short(0x0112, d.orientation),
            long(0x8769, exif_ptr),
        ];
        if let Some(p) = gps_ptr {
            v.push(long(0x8825, p));
        }
        v
    };
    let exif_ifd = vec![ascii(0x9003, d.date_time_original)];
    let gps_ifd = d.gps.map(|(lat, lon)| {
        vec![
            ascii(0x0001, if lat >= 0.0 { "N" } else { "S" }),
            rationals(0x0002, &dms(lat)),
            ascii(0x0003, if lon >= 0.0 { "E" } else { "W" }),
            rationals(0x0004, &dms(lon)),
        ]
    });

    let size0 = ifd_size(&ifd0(0, gps_ifd.as_ref().map(|_| 0)));
    let exif_off = 8 + size0;
    let gps_off = exif_off + ifd_size(&exif_ifd);

    let mut out = vec![b'I', b'I', 42, 0, 8, 0, 0, 0];
    write_ifd(
        &mut out,
        ifd0(exif_off as u32, gps_ifd.as_ref().map(|_| gps_off as u32)),
    );
    write_ifd(&mut out, exif_ifd);
    if let Some(g) = gps_ifd {
        write_ifd(&mut out, g);
    }
    out
}

/// Inserts an EXIF APP1 segment right after the JPEG SOI marker.
pub fn splice_into_jpeg(jpeg: &[u8], d: &ExifData) -> Vec<u8> {
    let tiff = build_tiff(d);
    let mut app1 = b"Exif\0\0".to_vec();
    app1.extend(tiff);
    let mut out = vec![0xFF, 0xD8, 0xFF, 0xE1];
    out.extend(((app1.len() + 2) as u16).to_be_bytes());
    out.extend(app1);
    out.extend(&jpeg[2..]);
    out
}
