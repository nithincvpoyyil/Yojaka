//! The path template mini-language (FR-40, FR-45, FR-49).
//!
//! `{year}/{month:02}/{name}` is parsed once into segments of tokens and then rendered for each
//! file. A segment whose placeholder has no value is dropped, so there are never empty folders;
//! a file without a date goes to `_undated`, one without any location to the "no location" folder.

use std::fmt;

use unicode_normalization::UnicodeNormalization;

use crate::model::MediaKind;
use crate::{Result, YojakaError};

pub const UNDATED_DIR: &str = "_undated";
/// Longest folder or file name Yojaka will produce (bytes; APFS allows 255).
const MAX_NAME_BYTES: usize = 200;

/// Ready-made templates: (label, template). Months are named (`Jan`), not numbered.
pub const PRESETS: [(&str, &str); 4] = [
    ("Date", "{year}/{month_name}/{name}"),
    ("Place", "{country}/{city}/{name}"),
    (
        "Place + Date",
        "{country}/{city}/{year}-{month_name}/{name}",
    ),
    ("Kind + Date", "{kind}/{year}/{month_name}/{name}"),
];

/// What `{month_name}` gives, January first.
const MONTH_NAMES: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Year,
    Month,
    MonthName,
    Day,
    Camera,
    Make,
    Model,
    Format,
    Ext,
    Name,
    Stem,
    Parent,
    Kind,
    Country,
    CountryCode,
    State,
    District,
    City,
}

const NAMES: [(&str, Field); 18] = [
    ("year", Field::Year),
    ("month", Field::Month),
    ("month_name", Field::MonthName),
    ("day", Field::Day),
    ("camera", Field::Camera),
    ("make", Field::Make),
    ("model", Field::Model),
    ("format", Field::Format),
    ("ext", Field::Ext),
    ("name", Field::Name),
    ("stem", Field::Stem),
    ("parent", Field::Parent),
    ("kind", Field::Kind),
    ("country", Field::Country),
    ("country_code", Field::CountryCode),
    ("state", Field::State),
    ("district", Field::District),
    ("city", Field::City),
];

impl Field {
    fn is_date(self) -> bool {
        matches!(
            self,
            Field::Year | Field::Month | Field::MonthName | Field::Day
        )
    }
    fn is_place(self) -> bool {
        matches!(
            self,
            Field::Country | Field::CountryCode | Field::State | Field::District | Field::City
        )
    }
    fn is_name(self) -> bool {
        matches!(self, Field::Name | Field::Stem)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    Lit(String),
    Field { field: Field, pad: usize },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Template {
    src: String,
    segments: Vec<Vec<Token>>,
}

impl fmt::Display for Template {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.src)
    }
}

/// A calendar date as recorded in the file (no time zone applied).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ymd {
    pub year: i32,
    pub month: u8,
    pub day: u8,
}

/// The place of a file after the precision guard: `None` parts are left out of the path.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Where {
    pub country: Option<String>,
    pub country_code: Option<String>,
    pub state: Option<String>,
    pub district: Option<String>,
    pub city: Option<String>,
}

/// Everything a template can ask about one file.
#[derive(Debug, Clone)]
pub struct Facts {
    /// File name with extension.
    pub name: String,
    pub parent: String,
    pub kind: MediaKind,
    pub format: Option<String>,
    pub camera_make: Option<String>,
    pub camera_model: Option<String>,
    pub date: Option<Ymd>,
    /// `None`: the file has no location at all.
    pub place: Option<Where>,
}

impl Facts {
    fn stem_ext(&self) -> (&str, &str) {
        match self.name.rfind('.') {
            Some(i) if i > 0 => (&self.name[..i], &self.name[i + 1..]),
            _ => (&self.name, ""),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rendered {
    /// Folders then the file name. Never empty; no segment is empty or contains `/`.
    pub segments: Vec<String>,
    pub undated: bool,
    pub no_location: bool,
}

impl Template {
    pub fn parse(src: &str) -> Result<Self> {
        let bad = |m: String| YojakaError::Internal(format!("template \"{src}\": {m}"));
        let src = src.trim();
        if src.is_empty() {
            return Err(bad("it is empty".into()));
        }
        if src.starts_with('/') || src.starts_with('~') {
            return Err(bad("it must be relative to the destination folder".into()));
        }
        let mut segments = Vec::new();
        for raw in src.split('/') {
            if raw.is_empty() {
                return Err(bad("it has an empty folder name (a doubled /)".into()));
            }
            if raw == "." || raw == ".." {
                return Err(bad("it may not contain . or .. folders".into()));
            }
            segments.push(parse_segment(raw).map_err(bad)?);
        }
        let last = segments.last().map(Vec::as_slice).unwrap_or_default();
        let has_name = last
            .iter()
            .any(|t| matches!(t, Token::Field { field, .. } if field.is_name()));
        if !has_name {
            return Err(bad(
                "the last part must contain {name} (or {stem}), or files would overwrite each other"
                    .into(),
            ));
        }
        if segments[..segments.len() - 1]
            .iter()
            .flatten()
            .any(|t| matches!(t, Token::Field { field, .. } if field.is_name()))
        {
            return Err(bad("{name} and {stem} belong in the last part only".into()));
        }
        Ok(Self {
            src: src.to_string(),
            segments,
        })
    }

    pub fn source(&self) -> &str {
        &self.src
    }

    /// True if any placeholder asks for a place.
    pub fn uses_place(&self) -> bool {
        self.segments
            .iter()
            .flatten()
            .any(|t| matches!(t, Token::Field { field, .. } if field.is_place()))
    }

    pub fn uses_date(&self) -> bool {
        self.segments
            .iter()
            .flatten()
            .any(|t| matches!(t, Token::Field { field, .. } if field.is_date()))
    }

    /// Renders the path for one file. `no_location_dir` names the folder for files without a place.
    pub fn render(&self, f: &Facts, no_location_dir: &str) -> Rendered {
        let mut out: Vec<String> = Vec::new();
        let (mut undated, mut no_location) = (false, false);
        let last = self.segments.len() - 1;
        for (i, seg) in self.segments.iter().enumerate() {
            let wants_date = seg
                .iter()
                .any(|t| matches!(t, Token::Field { field, .. } if field.is_date()));
            let wants_place = seg
                .iter()
                .any(|t| matches!(t, Token::Field { field, .. } if field.is_place()));
            let mut marker: Option<&str> = None;
            if wants_date && f.date.is_none() {
                marker = Some(UNDATED_DIR);
                undated = true;
            } else if wants_place && f.place.is_none() {
                marker = Some(no_location_dir);
                no_location = true;
            }
            if let Some(m) = marker {
                if i != last {
                    let m = sanitize(m);
                    if out.last() != Some(&m) {
                        out.push(m);
                    }
                    continue;
                }
                // The file-name part itself needs a date or place: put the marker in front.
                let m = sanitize(m);
                if out.last() != Some(&m) {
                    out.push(m);
                }
            }
            let mut text = String::new();
            let mut empty = false;
            for t in seg {
                match t {
                    Token::Lit(s) => text.push_str(s),
                    Token::Field { field, pad } => match value(*field, *pad, f) {
                        Some(v) => text.push_str(&v),
                        None => empty = true,
                    },
                }
            }
            let text = sanitize(&text);
            if i == last {
                // The file name is never dropped; an unusable one falls back to the original.
                out.push(if empty || text.is_empty() {
                    sanitize(&f.name)
                } else {
                    text
                });
            } else if !empty && !text.is_empty() {
                out.push(text);
            }
        }
        Rendered {
            segments: out,
            undated,
            no_location,
        }
    }
}

fn parse_segment(raw: &str) -> std::result::Result<Vec<Token>, String> {
    let mut tokens = Vec::new();
    let mut lit = String::new();
    let mut chars = raw.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' if chars.peek() == Some(&'{') => {
                chars.next();
                lit.push('{');
            }
            '}' if chars.peek() == Some(&'}') => {
                chars.next();
                lit.push('}');
            }
            '{' => {
                let mut body = String::new();
                loop {
                    match chars.next() {
                        Some('}') => break,
                        Some(ch) => body.push(ch),
                        None => return Err(format!("an opening brace is not closed in \"{raw}\"")),
                    }
                }
                if !lit.is_empty() {
                    tokens.push(Token::Lit(std::mem::take(&mut lit)));
                }
                let (name, spec) = match body.split_once(':') {
                    Some((n, s)) => (n, Some(s)),
                    None => (body.as_str(), None),
                };
                let field = NAMES
                    .iter()
                    .find(|(n, _)| *n == name)
                    .map(|(_, f)| *f)
                    .ok_or_else(|| {
                        let all: Vec<&str> = NAMES.iter().map(|(n, _)| *n).collect();
                        format!(
                            "unknown placeholder {{{name}}}; use one of: {}",
                            all.join(", ")
                        )
                    })?;
                let pad = match spec {
                    None => 0,
                    Some(s) => {
                        let ok_field = matches!(field, Field::Month | Field::Day | Field::Year);
                        let digits = s.strip_prefix('0').unwrap_or(s);
                        match digits.parse::<usize>() {
                            Ok(n) if ok_field && s.starts_with('0') && n <= 8 => n,
                            _ => {
                                return Err(format!(
                                    "\"{{{body}}}\": only month, day and year take a width like :02"
                                ));
                            }
                        }
                    }
                };
                tokens.push(Token::Field { field, pad });
            }
            '}' => return Err(format!("a stray closing brace in \"{raw}\"")),
            c => lit.push(c),
        }
    }
    if !lit.is_empty() {
        tokens.push(Token::Lit(lit));
    }
    Ok(tokens)
}

fn value(field: Field, pad: usize, f: &Facts) -> Option<String> {
    let num = |n: i64| format!("{n:0pad$}");
    let nonempty = |s: &str| (!s.trim().is_empty()).then(|| s.to_string());
    match field {
        Field::Year => f.date.map(|d| num(i64::from(d.year))),
        Field::Month => f.date.map(|d| num(i64::from(d.month))),
        Field::MonthName => f
            .date
            .and_then(|d| MONTH_NAMES.get(usize::from(d.month).checked_sub(1)?))
            .map(|m| (*m).to_string()),
        Field::Day => f.date.map(|d| num(i64::from(d.day))),
        Field::Make => f.camera_make.as_deref().and_then(nonempty),
        Field::Model => f.camera_model.as_deref().and_then(nonempty),
        Field::Camera => {
            let make = f.camera_make.as_deref().unwrap_or("").trim();
            let model = f.camera_model.as_deref().unwrap_or("").trim();
            let label =
                if !model.is_empty() && model.to_lowercase().starts_with(&make.to_lowercase()) {
                    model.to_string()
                } else {
                    format!("{make} {model}").trim().to_string()
                };
            nonempty(&label)
        }
        Field::Format => f.format.as_deref().and_then(nonempty),
        Field::Ext => nonempty(&f.stem_ext().1.to_lowercase()),
        Field::Name => nonempty(&f.name),
        Field::Stem => nonempty(f.stem_ext().0),
        Field::Parent => nonempty(&f.parent),
        Field::Kind => Some(
            match f.kind {
                MediaKind::Image => "Photos",
                MediaKind::Video => "Videos",
            }
            .into(),
        ),
        Field::Country => f.place.as_ref()?.country.clone(),
        Field::CountryCode => f.place.as_ref()?.country_code.clone(),
        Field::State => f.place.as_ref()?.state.clone(),
        Field::District => f.place.as_ref()?.district.clone(),
        Field::City => f.place.as_ref()?.city.clone(),
    }
}

/// One path component made safe for the file system: no `/` or `:`, no control characters, no
/// leading dot (it would hide the folder), trimmed, NFC, and not longer than APFS likes.
pub fn sanitize(s: &str) -> String {
    let cleaned: String = s
        .nfc()
        .map(|c| match c {
            '/' | ':' | '\\' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    let mut t = cleaned.trim().trim_start_matches('.').trim().to_string();
    if t.len() > MAX_NAME_BYTES {
        // Keep the end (the extension) of a long name.
        let ext_start = t
            .rfind('.')
            .filter(|i| t.len() - i <= 12)
            .unwrap_or(t.len());
        let (stem, ext) = t.split_at(ext_start);
        let mut cut = MAX_NAME_BYTES.saturating_sub(ext.len());
        while !stem.is_char_boundary(cut) {
            cut -= 1;
        }
        t = format!("{}{ext}", &stem[..cut]);
    }
    t
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn facts(name: &str) -> Facts {
        Facts {
            name: name.into(),
            parent: "Holiday".into(),
            kind: MediaKind::Image,
            format: Some("jpeg".into()),
            camera_make: Some("SONY".into()),
            camera_model: Some("SONY ILCE-7M3".into()),
            date: Some(Ymd {
                year: 2023,
                month: 8,
                day: 5,
            }),
            place: Some(Where {
                country: Some("India".into()),
                country_code: Some("IN".into()),
                state: Some("Goa".into()),
                district: Some("North Goa".into()),
                city: Some("Panaji".into()),
            }),
        }
    }

    fn render(t: &str, f: &Facts) -> String {
        Template::parse(t)
            .unwrap()
            .render(f, "No Location")
            .segments
            .join("/")
    }

    #[test]
    fn the_default_template_and_padding() {
        let f = facts("IMG_1.JPG");
        assert_eq!(render("{year}/{month:02}/{name}", &f), "2023/08/IMG_1.JPG");
        assert_eq!(
            render("{year}/{month}/{day:02}-{stem}.{ext}", &f),
            "2023/8/05-IMG_1.jpg"
        );
    }

    #[test]
    fn month_names_are_short_english_names() {
        let mut f = facts("a.jpg");
        assert_eq!(render("{year}/{month_name}/{name}", &f), "2023/Aug/a.jpg");
        for (month, name) in [(1, "Jan"), (12, "Dec")] {
            f.date = Some(Ymd {
                year: 2023,
                month,
                day: 1,
            });
            assert_eq!(render("{month_name}/{name}", &f), format!("{name}/a.jpg"));
        }
        // An impossible month drops the folder instead of inventing one.
        f.date = Some(Ymd {
            year: 2023,
            month: 13,
            day: 1,
        });
        assert_eq!(render("{year}/{month_name}/{name}", &f), "2023/a.jpg");
        // Without a date it is an undated file, like {month}.
        f.date = None;
        assert_eq!(render("{month_name}/{name}", &f), "_undated/a.jpg");
        assert!(Template::parse("{month_name:02}/{name}").is_err());
    }

    #[test]
    fn camera_does_not_repeat_the_make() {
        assert_eq!(
            render("{camera}/{name}", &facts("a.jpg")),
            "SONY ILCE-7M3/a.jpg"
        );
        let mut f = facts("a.jpg");
        f.camera_model = Some("Pixel 7".into());
        f.camera_make = Some("Google".into());
        assert_eq!(render("{camera}/{name}", &f), "Google Pixel 7/a.jpg");
    }

    #[test]
    fn kind_and_format_and_parent() {
        let mut f = facts("a.mov");
        f.kind = MediaKind::Video;
        assert_eq!(
            render("{kind}/{format}/{parent}/{name}", &f),
            "Videos/jpeg/Holiday/a.mov"
        );
    }

    #[test]
    fn an_empty_placeholder_drops_its_segment() {
        let mut f = facts("a.jpg");
        f.camera_make = None;
        f.camera_model = None;
        assert_eq!(render("{year}/{camera}/{name}", &f), "2023/a.jpg");
        let mut f = facts("a.jpg");
        f.place.as_mut().unwrap().city = None;
        assert_eq!(
            render("{country}/{city}/{year}/{name}", &f),
            "India/2023/a.jpg"
        );
    }

    #[test]
    fn no_date_goes_to_undated_once() {
        let mut f = facts("a.jpg");
        f.date = None;
        let r = Template::parse("{year}/{month:02}/{name}")
            .unwrap()
            .render(&f, "X");
        assert_eq!(r.segments, ["_undated", "a.jpg"]);
        assert!(r.undated && !r.no_location);
    }

    #[test]
    fn no_location_goes_to_the_named_folder_once() {
        let mut f = facts("a.jpg");
        f.place = None;
        let r = Template::parse("{country}/{city}/{year}/{name}")
            .unwrap()
            .render(&f, "No Location");
        assert_eq!(r.segments, ["No Location", "2023", "a.jpg"]);
        assert!(r.no_location);
        // A template without place fields never produces the marker.
        let r = Template::parse("{year}/{name}")
            .unwrap()
            .render(&f, "No Location");
        assert!(!r.no_location);
    }

    #[test]
    fn values_are_made_safe() {
        assert_eq!(sanitize("a/b:c"), "a_b_c");
        assert_eq!(sanitize("  ..hidden "), "hidden");
        assert_eq!(sanitize("e\u{0301}"), "\u{e9}", "NFC");
        let long = format!("{}.jpg", "x".repeat(400));
        let s = sanitize(&long);
        assert!(s.len() <= 200 && s.ends_with(".jpg"));
        let mut f = facts("a.jpg");
        f.place.as_mut().unwrap().city = Some("Foo/Bar".into());
        assert_eq!(render("{city}/{name}", &f), "Foo_Bar/a.jpg");
    }

    #[test]
    fn literals_and_escaped_braces_are_kept() {
        assert_eq!(
            render("Photos {year}/{name}", &facts("a.jpg")),
            "Photos 2023/a.jpg"
        );
        assert_eq!(render("{{x}}/{name}", &facts("a.jpg")), "{x}/a.jpg");
    }

    #[test]
    fn bad_templates_are_explained() {
        for (t, needle) in [
            ("", "empty"),
            ("/abs/{name}", "relative"),
            ("{year}//{name}", "doubled"),
            ("../{name}", ".."),
            ("{year}", "{name}"),
            ("{nope}/{name}", "unknown placeholder"),
            ("{year/{name}", "not closed"),
            ("{name}/{year}", "last part"),
            ("{city:02}/{name}", "width"),
            ("a}/{name}", "stray"),
        ] {
            let e = Template::parse(t).unwrap_err().to_string();
            assert!(e.contains(needle), "{t}: {e}");
        }
    }

    #[test]
    fn presets_all_parse() {
        for (_, t) in PRESETS {
            Template::parse(t).unwrap();
        }
    }

    use proptest::prelude::*;

    proptest! {
        /// Whatever the file is called and wherever it was taken, a rendered path is a safe,
        /// relative one: no empty part, no `/` inside a part, no `.` or `..`, no hidden folder.
        #[test]
        fn rendered_paths_are_always_safe(
            name in "\\PC{0,40}",
            city in proptest::option::of("\\PC{0,30}"),
            make in proptest::option::of("\\PC{0,20}"),
            has_date in any::<bool>(),
            has_place in any::<bool>(),
            tpl in prop::sample::select(vec![
                "{year}/{month:02}/{name}",
                "{country}/{city}/{year}-{month_name}/{name}",
                "{kind}/{camera}/{parent}/{stem}.{ext}",
                "{state}/{district}/{day:02}/{name}",
            ]),
        ) {
            let mut f = facts("x.jpg");
            f.name = format!("{name}.jpg");
            f.camera_make = make;
            f.date = has_date.then_some(Ymd { year: 2021, month: 3, day: 9 });
            f.place = has_place.then(|| Where { city, country: Some("India".into()), ..Where::default() });
            let r = Template::parse(tpl).unwrap().render(&f, "No Location");
            prop_assert!(!r.segments.is_empty());
            for seg in &r.segments {
                prop_assert!(!seg.is_empty());
                prop_assert!(!seg.contains('/') && !seg.contains(':'));
                prop_assert!(seg != "." && seg != "..");
                prop_assert!(!seg.starts_with('.'));
                prop_assert!(seg.len() <= 200);
            }
        }
    }
}
