//! A Han webfont a theme hosts whole, published as slices.
//!
//! A Han face is several megabytes; a page needs a few hundred of its
//! characters. Google Fonts serves such faces cut into about a hundred and
//! twenty slices, each declared with its `unicode-range`, so a browser fetches
//! only the slices a page's text touches. This does the same for a font file
//! in the theme folder: one ordinary `@font-face` with no `unicode-range`
//! becomes one rule per slice.
//!
//! The cut comes from the font and the site's language alone, never from the
//! site's text, so the stylesheet is known before any page renders and does
//! not change when the text does. A slice is named by its inputs (the font's
//! bytes, the table, the slice and [`ENCODER_VERSION`]), so its address is
//! known before it is encoded; the encoding runs after the pages are written
//! and is kept in a cache every site on the machine shares.
//!
//! The tables are Google Fonts' CJK slicing strategies, from
//! github.com/googlefonts/nam-files (Apache-2.0), packed by
//! `scripts/gen-cjk-slices.py`.

mod instance;

use std::sync::LazyLock;

/// Raised whenever the cut, the subset or the WOFF2 encoding changes a
/// slice's bytes, so every slice gets a new address and no browser keeps an
/// old copy.
pub const ENCODER_VERSION: u32 = 5;

/// Brotli quality of every slice. A slice is encoded once per font, so the
/// slowest setting pays for itself on the first visit.
const BROTLI_QUALITY: u8 = 11;

/// A font mapping fewer Han ideographs than this is published whole: slicing
/// a Latin face with a few CJK marks buys nothing.
const MIN_HAN_CODEPOINTS: usize = 1000;

/// Code points a table does not list (Extension A and B, kana and symbols a
/// Chinese table leaves out) go to extra slices of this many, by code point:
/// the size of the tables' own bins.
const OVERFLOW_SLICE_LEN: usize = 135;

/// The table names, in the order `cjk-slices.bin.br` packs them.
const STRATEGIES: [&str; 5] = ["traditional-chinese", "hongkong-chinese", "simplified-chinese", "japanese", "korean"];

/// The table for a site whose language names no CJK script.
const DEFAULT_STRATEGY: usize = 2;

struct Strategy {
    /// Frequency subsets by priority, then bins by code point. The first holds
    /// ASCII, so every page loads it.
    subsets: Vec<Vec<u32>>,
    /// How many of `subsets` are frequency subsets.
    frequency: usize,
}

const TABLE_BYTES: &[u8] = include_bytes!("../../assets/fonts/cjk-slices.bin.br");

static TABLES: LazyLock<Vec<Strategy>> = LazyLock::new(|| {
    let mut packed = Vec::new();
    brotli::BrotliDecompress(&mut &TABLE_BYTES[..], &mut packed).expect("cjk-slices.bin.br is brotli");
    decode(&packed)
});

/// One hash per table, named in every slice's address, so a new cut of a
/// table gets new addresses and a cut of another table keeps them.
static TABLE_HASHES: LazyLock<Vec<String>> = LazyLock::new(|| {
    TABLES.iter().map(|table| {
        let mut bytes = (table.frequency as u32).to_le_bytes().to_vec();
        for subset in &table.subsets {
            bytes.extend(subset.iter().chain([&u32::MAX]).flat_map(|cp| cp.to_le_bytes()));
        }
        crate::build::assets::paths::compute_binary_hash(&bytes)
    }).collect()
});

/// The OpenType feature each `font-variant-*` keyword turns on, for those
/// beyond the subsetter's default list.
const VARIANT_FEATURES: &[(&str, &[&[u8; 4]])] = &[
    ("ruby", &[b"ruby"]), ("full-width", &[b"fwid"]), ("proportional-width", &[b"pwid"]),
    ("jis78", &[b"jp78"]), ("jis83", &[b"jp83"]), ("jis90", &[b"jp90"]), ("jis04", &[b"jp04"]),
    ("simplified", &[b"smpl"]), ("traditional", &[b"trad"]),
    ("discretionary-ligatures", &[b"dlig"]), ("historical-ligatures", &[b"hlig"]), ("historical-forms", &[b"hist"]),
    ("lining-nums", &[b"lnum"]), ("oldstyle-nums", &[b"onum"]), ("proportional-nums", &[b"pnum"]), ("tabular-nums", &[b"tnum"]),
    ("stacked-fractions", &[b"afrc"]), ("ordinal", &[b"ordn"]), ("slashed-zero", &[b"zero"]),
    ("small-caps", &[b"smcp"]), ("all-small-caps", &[b"smcp", b"c2sc"]), ("petite-caps", &[b"pcap"]), ("all-petite-caps", &[b"pcap", b"c2pc"]),
    ("unicase", &[b"unic"]), ("titling-caps", &[b"titl"]), ("sub", &[b"subs"]), ("super", &[b"sups"]),
];

/// The features `css` turns on with `font-variant-*` or names in
/// `font-feature-settings`. Only stylesheets are read: a feature that only an
/// inline `style` attribute asks for is not kept in the slices.
fn requested_features(css: &str) -> Vec<[u8; 4]> {
    let mut out = Vec::new();
    for rule in crate::build::theme_lint::style_rules(css) {
        for (property, value) in rule.declarations() {
            let value = value.to_ascii_lowercase();
            if property.starts_with("font-variant") {
                for word in value.split(|c: char| c.is_whitespace() || c == ',') {
                    out.extend(VARIANT_FEATURES.iter().filter(|(k, _)| *k == word).flat_map(|(_, tags)| tags.iter().map(|t| **t)));
                }
            } else if property == "font-feature-settings" {
                for quoted in value.split(['"', '\'']).skip(1).step_by(2) {
                    if let Ok(tag) = <[u8; 4]>::try_from(quoted.as_bytes()) {
                        out.push(tag);
                    }
                }
            }
        }
    }
    out
}

/// moss's own stylesheets, every partial included whether it ships or not,
/// so a slice's address does not follow which pages a site has.
static MOSS_FEATURES: LazyLock<Vec<[u8; 4]>> = LazyLock::new(|| {
    let sheets = std::iter::once(crate::build::page::shell::DEFAULT_CSS).chain(super::stylesheet::CSS_PARTIALS.iter().map(|p| p.source));
    sheets.flat_map(requested_features).collect()
});

/// The features a slice keeps beyond the subsetter's default list: those the
/// theme's stylesheet or moss's turn on, sorted.
pub fn kept_features(theme_css: &str) -> Vec<[u8; 4]> {
    let mut features: Vec<[u8; 4]> = requested_features(theme_css).into_iter().chain(MOSS_FEATURES.iter().copied()).collect();
    features.sort_unstable();
    features.dedup();
    features
}

fn decode(mut bytes: &[u8]) -> Vec<Strategy> {
    let mut next = || {
        let (mut value, mut shift) = (0u32, 0);
        loop {
            let (byte, rest) = bytes.split_first().expect("cjk-slices.bin.br is truncated");
            bytes = rest;
            value |= u32::from(byte & 0x7f) << shift;
            if byte & 0x80 == 0 {
                return value;
            }
            shift += 7;
        }
    };
    let deltas = |n: u32, next: &mut dyn FnMut() -> u32| {
        let mut at = 0;
        (0..n).map(|_| { at += next(); at }).collect::<Vec<u32>>()
    };
    (0..next())
        .map(|_| {
            let count = next();
            let frequency = next() as usize;
            Strategy { subsets: (0..count).map(|_| { let n = next(); deltas(n, &mut next) }).collect(), frequency }
        })
        .collect()
}

/// Which table a site's language picks. Only the language decides: choosing
/// by the text would re-cut every published slice when the text tipped over.
pub fn strategy_for_language(lang: &str) -> usize {
    let lang = lang.to_ascii_lowercase().replace('_', "-");
    let mut parts = lang.split('-');
    let primary = parts.next().unwrap_or("");
    let rest: Vec<&str> = parts.collect();
    let has = |tag: &str| rest.contains(&tag);
    match primary {
        "ja" => 3,
        "ko" => 4,
        "zh" if has("hk") || has("mo") => 1,
        "zh" if has("hant") || has("tw") => 0,
        _ => DEFAULT_STRATEGY,
    }
}

/// One slice of one font: what to name it by and what goes in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Slice {
    pub id: u16,
    pub codepoints: Vec<u32>,
}

/// Cut the code points a font maps into slices under `strategy`. Every code
/// point lands in exactly one slice; slices the font has nothing for are left
/// out. U+0020 goes in the first slice whatever the table says: a line takes
/// its height from the first face whose range holds the space, so without it
/// every line would sit by the fallback font's metrics.
pub fn plan(font_codepoints: &[u32], strategy: usize) -> Vec<Slice> {
    let table = &TABLES[strategy];
    let mut slice_of: std::collections::HashMap<u32, usize> = std::collections::HashMap::new();
    for (i, subset) in table.subsets.iter().enumerate() {
        slice_of.extend(subset.iter().map(|&cp| (cp, i)));
    }
    slice_of.insert(0x20, 0);
    let mut slices: std::collections::BTreeMap<usize, Vec<u32>> = std::collections::BTreeMap::new();
    let mut overflow = Vec::new();
    for &cp in font_codepoints {
        match slice_of.get(&cp) {
            Some(&id) => slices.entry(id).or_default().push(cp),
            None => overflow.push(cp),
        }
    }
    overflow.sort_unstable();
    overflow.dedup();
    for (n, chunk) in overflow.chunks(OVERFLOW_SLICE_LEN).enumerate() {
        slices.insert(table.subsets.len() + n, chunk.to_vec());
    }
    slices
        .into_iter()
        .map(|(id, mut codepoints)| {
            codepoints.sort_unstable();
            codepoints.dedup();
            Slice { id: id as u16, codepoints }
        })
        .collect()
}

/// `slices` in the order their rules are written, each with its
/// `unicode-range`. A browser checks faces whose ranges overlap last-written
/// first and fetches only the first that covers a character, so the slices
/// are written lowest priority first (bins, then overflow, then frequency
/// subsets), and a range runs on across code points the font maps and a later
/// slice owns. That folds the gaps between a slice's members, where most of
/// the stylesheet's bytes would go. A range never covers a code point the
/// font lacks, which would fetch a slice for a character it cannot draw.
pub fn declared(mut slices: Vec<Slice>, strategy: usize) -> Vec<(Slice, String)> {
    let table = &TABLES[strategy];
    let rank = |id: u16| {
        let id = usize::from(id);
        if id < table.frequency { (0, id) } else if id >= table.subsets.len() { (1, id) } else { (2, id) }
    };
    slices.sort_by_key(|s| rank(s.id));
    // Every mapped code point with the priority of its slice (0 the highest).
    let mut owned: Vec<(u32, usize)> = slices.iter().enumerate().flat_map(|(p, s)| s.codepoints.iter().map(move |&cp| (cp, p))).collect();
    owned.sort_unstable();
    let at = |cp: u32| owned.partition_point(|&(c, _)| c < cp);
    let rules: Vec<String> = slices
        .iter()
        .enumerate()
        .map(|(p, slice)| {
            let mut runs: Vec<(u32, u32)> = Vec::new();
            for &cp in &slice.codepoints {
                if let Some(run) = runs.last_mut() {
                    let (from, to) = (at(run.1) + 1, at(cp));
                    if to - from == (cp - run.1 - 1) as usize && owned[from..to].iter().all(|&(_, q)| q < p) {
                        run.1 = cp;
                        continue;
                    }
                }
                runs.push((cp, cp));
            }
            runs.iter().map(|&(a, b)| if a == b { format!("U+{a:X}") } else { format!("U+{a:X}-{b:X}") }).collect::<Vec<_>>().join(",")
        })
        .collect();
    slices.into_iter().zip(rules).rev().collect()
}

/// Why a font is published whole instead of sliced.
#[derive(Debug, PartialEq, Eq)]
pub enum Whole {
    /// Not a TrueType or OpenType file moss can read.
    Unreadable,
    /// Too few Han ideographs to be worth slicing; nobody is told.
    NotHan,
    /// The font's licence bits (OS/2 `fsType`) forbid subsetting or embedding.
    LicenceForbids,
}

/// The code points `bytes` maps, if it is a Han font whose licence allows
/// subsetting.
pub fn sliceable_codepoints(bytes: &[u8]) -> Result<Vec<u32>, Whole> {
    let face = ttf_parser::Face::parse(bytes, 0).map_err(|_| Whole::Unreadable)?;
    let mut codepoints = Vec::new();
    for subtable in face.tables().cmap.into_iter().flat_map(|cmap| cmap.subtables) {
        if subtable.is_unicode() {
            subtable.codepoints(|cp| codepoints.push(cp));
        }
    }
    codepoints.sort_unstable();
    codepoints.dedup();
    let han = codepoints.iter().filter(|&&cp| (0x4e00..=0x9fff).contains(&cp)).count();
    if han < MIN_HAN_CODEPOINTS {
        return Err(Whole::NotHan);
    }
    let restricted = matches!(face.permissions(), Some(ttf_parser::Permissions::Restricted));
    if restricted || !face.is_subsetting_allowed() {
        return Err(Whole::LicenceForbids);
    }
    Ok(codepoints)
}

/// The slice's WOFF2 bytes: [`subset`], pinned at `weight` when one is given,
/// with the glyf transform wherever a decoder can rebuild the file.
pub fn encode(bytes: &[u8], codepoints: &[u32], weight: Option<u16>, features: &[[u8; 4]]) -> Result<Vec<u8>, String> {
    let mut sfnt = subset(bytes, codepoints, features)?;
    if let Some(weight) = weight {
        sfnt = instance::instance(&sfnt, &[(WGHT, f32::from(weight))]).map_err(|e| e.to_string())?;
    }
    let options = ttf2woff2::EncodeOptions {
        quality: ttf2woff2::BrotliQuality::from(BROTLI_QUALITY),
        transform_glyf_loca: transform_is_safe(&sfnt),
        ..Default::default()
    };
    ttf2woff2::encode_with_options(&sfnt, options).map_err(|e| e.to_string())
}

/// Chromium's decoder rebuilds `loca` in the format the font names and pads
/// each glyph to four bytes, rejecting the file when a short `loca` (offsets
/// halved into 16 bits) cannot hold the result.
fn transform_is_safe(sfnt: &[u8]) -> bool {
    use read_fonts::{types::Tag, TableProvider};
    let Ok(font) = read_fonts::FontRef::new(sfnt) else { return false };
    let (Ok(head), Ok(maxp)) = (font.head(), font.maxp()) else { return false };
    let glyf = font.table_data(Tag::new(b"glyf")).map_or(0, |data| data.len());
    head.index_to_loc_format() == 1 || short_loca_holds(glyf, maxp.num_glyphs())
}

pub(crate) fn short_loca_holds(glyf_len: usize, num_glyphs: u16) -> bool {
    glyf_len + 3 * usize::from(num_glyphs) <= 0x1FFFE
}

/// The font cut to `codepoints` as an sfnt, keeping the subsetter's default
/// layout features (vertical forms among them) plus `features`, and the font's
/// own vertical metrics, and dropping its hinting, which no browser applies to
/// a Han face. Every feature would double a kana slice: `aalt` alone reaches
/// hundreds of alternate glyphs no page draws.
pub fn subset(bytes: &[u8], codepoints: &[u32], features: &[[u8; 4]]) -> Result<Vec<u8>, String> {
    use read_fonts::collections::IntSet;
    use read_fonts::types::{GlyphId, NameId, Tag};
    let font = read_fonts::FontRef::new(bytes).map_err(|e| e.to_string())?;
    let drop: IntSet<Tag> = skera::DEFAULT_DROP_TABLES.iter().copied().collect();
    let mut every_script = IntSet::<Tag>::empty();
    every_script.invert();
    let kept: IntSet<Tag> = skera::DEFAULT_LAYOUT_FEATURES.iter().copied().chain(features.iter().map(|t| Tag::new(t))).collect();
    let mut names = IntSet::<NameId>::empty();
    names.insert_range(NameId::from(0)..=NameId::from(14));
    let mut langs = IntSet::<u16>::empty();
    langs.insert(0x0409);
    let unicodes: IntSet<u32> = codepoints.iter().copied().collect();
    let plan = skera::Plan::new(&IntSet::<GlyphId>::empty(), &unicodes, &font, skera::SubsetFlags::SUBSET_FLAGS_NO_HINTING, &drop, &every_script, &kept, &names, &langs);
    skera::subset_font(&font, &plan).map_err(|e| format!("{e:?}"))
}

/// The name of a slice, from its inputs only: one hash of the font, the
/// table, the encoder and the kept features, then the pinned weight (`None`
/// keeps the font's own axes) and the slice id in plain text. One hash shared
/// by every rule of a font is what lets gzip fold the rules.
pub fn slice_name(font_hash: &str, strategy: usize, id: u16, weight: Option<u16>, features: &[[u8; 4]]) -> String {
    let features: String = features.iter().map(|t| String::from_utf8_lossy(t)).collect();
    let key = format!("{font_hash}\0{}\0{}\0{ENCODER_VERSION}\0{features}", STRATEGIES[strategy], TABLE_HASHES[strategy]);
    let set = crate::build::assets::paths::compute_binary_hash(key.as_bytes());
    match weight {
        Some(w) => format!("{set}-{w}-{id}"),
        None => format!("{set}-{id}"),
    }
}

/// One slice to encode after paint: where it is published and what goes in it.
#[derive(Clone)]
pub struct SliceJob {
    pub path: crate::build::served_path::ServedPath,
    /// [`slice_name`]: the machine cache's key as well as the file's name.
    pub name: String,
    pub font: std::sync::Arc<Vec<u8>>,
    pub codepoints: Vec<u32>,
    pub weight: Option<u16>,
    /// [`kept_features`], shared by every slice of a stylesheet.
    pub features: std::sync::Arc<Vec<[u8; 4]>>,
}

impl std::fmt::Debug for SliceJob {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SliceJob").field("path", &self.path.as_str()).field("codepoints", &self.codepoints.len()).field("weight", &self.weight).finish()
    }
}

/// `css` with each sliceable face replaced by its slice rules, the slices to
/// encode, and a note per face published whole for its licence.
pub struct Sliced {
    pub css: String,
    pub jobs: Vec<SliceJob>,
    pub notes: Vec<String>,
}

/// One `@font-face` to slice: where it sits in the stylesheet and what it says.
struct Face {
    span: std::ops::Range<usize>,
    written: String,
    rel: String,
    /// Every descriptor but `src`, in the order written.
    kept: Vec<(String, String)>,
}

impl Face {
    /// What a face loading the same file must also share to be written with this one.
    fn group_key(&self) -> (&str, &str, Vec<&(String, String)>) {
        (&self.rel, &self.written, self.kept.iter().filter(|(p, _)| p != "font-weight").collect())
    }

    /// `font-weight` as written, if the face has one.
    fn weight(&self) -> Option<&str> {
        self.kept.iter().rev().find(|(p, _)| p == "font-weight").map(|(_, v)| v.as_str())
    }

    /// Where slice `name` is published, theme-relative, and the url the
    /// stylesheet loads it from, both beside the font.
    fn slice_paths(&self, name: &str) -> (String, String) {
        let (dir, file) = self.rel.rsplit_once('/').unwrap_or(("", &self.rel));
        let stem = file.rsplit_once('.').map_or(file, |(stem, _)| stem);
        let file = format!("{stem}.{name}.woff2");
        let beside = |dir: &str| if dir.is_empty() { file.clone() } else { format!("{dir}/{file}") };
        (beside(dir), beside(self.written.rsplit_once('/').map_or("", |(dir, _)| dir)))
    }

    /// The descriptors, with `font-weight` set to `pinned` when given.
    fn descriptors(&self, pinned: Option<u16>) -> String {
        let mut out: String = self.kept.iter().map(|(p, v)| match pinned.filter(|_| p == "font-weight") {
            Some(w) => format!("{p}:{w};"),
            None => format!("{p}:{v};"),
        }).collect();
        if let (Some(w), None) = (pinned, self.weight()) {
            out.push_str(&format!("font-weight:{w};"));
        }
        out
    }
}

/// Replace every `@font-face` in the theme's own `style.css` that has no
/// `unicode-range` and loads a `.ttf` or `.otf` file from the theme folder
/// with one rule per slice of that file. `read` returns a theme-relative
/// file's bytes. Each slice rule keeps the face's other descriptors.
///
/// A variable font whose one axis is `wght` is pinned at each face's single
/// weight, which is where a browser draws it. Faces loading the same file are
/// written together, a slice's weights side by side, so the repeated
/// `unicode-range` compresses away.
pub fn slice_faces(css: &str, strategy: usize, mut read: impl FnMut(&str) -> Option<Vec<u8>>) -> Sliced {
    let mut out = Sliced { css: String::with_capacity(css.len()), jobs: Vec::new(), notes: Vec::new() };
    // `font-variation-settings` sets the axes over the face's weight.
    let pinnable = !css.to_ascii_lowercase().contains("font-variation-settings");
    let features = std::sync::Arc::new(kept_features(css));
    let mut groups: Vec<Vec<Face>> = Vec::new();
    for rule in crate::build::theme_lint::style_rules(css) {
        if !rule.conditions.is_empty() || !rule.prelude.trim().eq_ignore_ascii_case("@font-face") {
            continue;
        }
        let declarations = rule.declarations();
        if declarations.iter().any(|(p, _)| p == "unicode-range") {
            continue;
        }
        let Some((written, rel)) = declarations.iter().rev().find(|(p, _)| p == "src").and_then(|(_, src)| font_url(src)) else { continue };
        let start = rule.start;
        let kept: Vec<(String, String)> = declarations.iter().filter(|(p, _)| p != "src").cloned().collect();
        let face = Face { span: start..start + rule.rule.len(), written, rel, kept };
        match groups.iter_mut().find(|g| g[0].group_key() == face.group_key()) {
            Some(group) => group.push(face),
            None => groups.push(vec![face]),
        }
    }
    let mut replaced: Vec<(std::ops::Range<usize>, String)> = Vec::new();
    for group in groups {
        let first = &group[0];
        let Some(bytes) = read(&first.rel) else { continue };
        let codepoints = match sliceable_codepoints(&bytes) {
            Ok(codepoints) => codepoints,
            Err(Whole::LicenceForbids) => {
                out.notes.push(format!("theme font {} is published whole: its licence does not allow subsetting", first.rel));
                continue;
            }
            Err(_) => continue,
        };
        // One slice is cut here, so a font the subsetter or the instancer
        // cannot take is published as written, not found slice by slice later.
        let probe = match subset(&bytes, &codepoints[..1], &features) {
            Ok(probe) => probe,
            Err(e) => {
                out.notes.push(format!("theme font {} is published whole: it could not be subset ({e})", first.rel));
                continue;
            }
        };
        let pins = pinnable && varies_by_weight_alone(&bytes) && match instance::check(&bytes).and_then(|()| instance::instance(&probe, &[(WGHT, 400.0)]).map(drop)) {
            Ok(()) => true,
            Err(why) => {
                out.notes.push(format!("theme font {} is published variable, not pinned at each weight: {why}", first.rel));
                false
            }
        };
        // Each distinct face: the weight it is pinned at, and its `font-weight` descriptor.
        let mut variants: Vec<(Option<u16>, String)> = Vec::new();
        for face in &group {
            let pinned = face_weight(face.weight()).filter(|_| pins);
            let variant = (pinned, face.descriptors(pinned));
            if !variants.contains(&variant) {
                variants.push(variant);
            }
        }
        let font_hash = crate::build::assets::paths::compute_binary_hash(&bytes);
        let font = std::sync::Arc::new(bytes);
        let mut rules = String::new();
        for (slice, range) in declared(plan(&codepoints, strategy), strategy) {
            for (weight, descriptor) in &variants {
                let name = slice_name(&font_hash, strategy, slice.id, *weight, &features);
                let (served, url) = first.slice_paths(&name);
                let Ok(path) = crate::build::served_path::ServedPath::for_theme_asset(&served) else { continue };
                rules.push_str(&format!("@font-face{{{descriptor}src:url(\"{url}\") format(\"woff2\");unicode-range:{range}}}\n"));
                out.jobs.push(SliceJob { path, name, font: font.clone(), codepoints: slice.codepoints.clone(), weight: *weight, features: features.clone() });
            }
        }
        let mut spans = group.iter().map(|f| f.span.clone());
        replaced.extend(spans.next().map(|span| (span, rules)));
        replaced.extend(spans.map(|span| (span, String::new())));
    }
    replaced.sort_by_key(|(span, _)| span.start);
    let mut copied = 0;
    for (span, rules) in replaced {
        out.css.push_str(css.get(copied..span.start).unwrap_or_default());
        out.css.push_str(&rules);
        copied = span.end;
    }
    out.css.push_str(css.get(copied..).unwrap_or_default());
    out
}

/// The one weight a face is drawn at: its `font-weight`, or 400 when it has
/// none. `None` for a range, which the variable font keeps.
fn face_weight(written: Option<&str>) -> Option<u16> {
    let Some(written) = written else { return Some(400) };
    match written.trim().to_ascii_lowercase().as_str() {
        "normal" => Some(400),
        "bold" => Some(700),
        number => number.parse::<f32>().ok().filter(|w| (1.0..=1000.0).contains(w)).map(|w| w.round() as u16),
    }
}

/// A variable font whose only axis is `wght`. Another axis (width, optical
/// size) is set by properties a pinned slice would ignore.
fn varies_by_weight_alone(bytes: &[u8]) -> bool {
    use read_fonts::TableProvider;
    let axes = read_fonts::FontRef::new(bytes).ok().and_then(|font| font.fvar().ok()).and_then(|fvar| fvar.axes().ok().map(|axes| axes.iter().map(|a| a.axis_tag()).collect::<Vec<_>>()));
    axes.is_some_and(|axes| axes == [WGHT])
}

const WGHT: read_fonts::types::Tag = read_fonts::types::Tag::new(b"wght");

/// The slice's bytes, from the machine's cache when any site has encoded it
/// before. [`slice_faces`] cut one slice of the font already, so an error
/// here is a slice unlike the rest.
pub fn encoded(job: &SliceJob) -> Result<Vec<u8>, String> {
    let cached = cache_dir().map(|dir| dir.join(format!("{}.woff2", job.name)));
    if let Some(bytes) = cached.as_ref().and_then(|path| std::fs::read(path).ok()) {
        return Ok(bytes);
    }
    let bytes = encode(&job.font, &job.codepoints, job.weight, &job.features).map_err(|e| format!("theme font slice {} could not be encoded ({e})", job.path.as_str()))?;
    if let Some(path) = cached {
        // Atomic, so a build reading at the same time never sees half a file;
        // a failed store costs only an encode on the next build.
        let _ = crate::build::io_utils::write_output(&path, &bytes);
    }
    Ok(bytes)
}

/// Shared by every site on the machine, so a second site with the same face
/// encodes nothing. A test binary keeps its own under the temporary folder.
fn cache_dir() -> Option<std::path::PathBuf> {
    if crate::infra::app_data::in_test_binary() {
        return Some(std::env::temp_dir().join("moss-font-slices"));
    }
    let local = crate::infra::app_data::AppRoots::running()?.local;
    Some(dirs::cache_dir()?.join(local.file_name()?).join("font-slices"))
}

/// The url a `src` value loads a TrueType or OpenType file from, as written
/// and as a theme-relative path; `None` when it names no such theme file.
fn font_url(src: &str) -> Option<(String, String)> {
    crate::build::emit::theme_css_assets::urls(src).into_iter().find_map(|url| {
        let path = url.split(['?', '#']).next().unwrap_or("");
        let ext = path.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase());
        if !matches!(ext.as_deref(), Some("ttf" | "otf")) || path.starts_with('/') || path.contains(':') {
            return None;
        }
        let decoded = percent_encoding::percent_decode_str(path).decode_utf8().ok()?;
        let rel = crate::build::emit::theme_css_assets::normalize(&format!("/{decoded}"))?;
        Some((path.to_string(), rel))
    })
}

#[cfg(test)]
#[path = "font_slices_tests.rs"]
mod tests;
