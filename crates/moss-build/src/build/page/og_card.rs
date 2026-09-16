//! Auto-generated 1200×630 Open Graph card.
//!
//! Renders an SVG from a template (title + site name + theme accent strip),
//! rasterizes to PNG via resvg, caches by content hash. Output is written
//! to `<output>/_moss/og/<hash>.png` and served at `/_moss/og/<hash>.png`.

use crate::build::io_utils::output_present;
use crate::build::served_path::ServedPath;
use crate::build::svg_util::is_valid_hex_color;
use sha2::{Digest, Sha256};
use std::path::Path;

/// Inputs that affect the rendered card. Any change here invalidates the cache.
pub struct CardInputs<'a> {
    pub title: &'a str,
    pub site_name: &'a str,
    pub bg_color: &'a str,      // e.g. "#faf8f5"
    pub fg_color: &'a str,      // e.g. "#1a1816"
    pub accent_color: &'a str,  // e.g. "#d97706"
    /// The page's resolved language. Only used to order the CJK glyph-fallback
    /// chain (Traditional-first vs Simplified-first); it changes no layout and
    /// no Latin rendering. The resolved script is part of the cache key because
    /// it changes the pixels of a CJK card.
    ///
    /// It orders the chain for the WHOLE card, title and site name together:
    /// usvg's fallback hook is `(char, used, db)` and cannot tell which text
    /// run it is in, so a Traditional site's name is set in a Simplified face on
    /// a `zh-hans` page. Per-run scripts would need a different seam.
    pub lang: crate::i18n::Language,
    /// The page's effective typesetting is vertical (`body[data-typesetting=
    /// "vertical"]`, from the page's or the site's `typesetting`). The card
    /// then sets its title top-to-bottom like a book's title strip instead of
    /// across; see [`build_vertical_svg`].
    pub vertical: bool,
    /// The page's own picture, when moss has decided to compose the card
    /// around it rather than hand the platform the file itself. See
    /// [`CardPlate`] and `build::page::cover::plate_decision`.
    pub plate: Option<CardPlate<'a>>,
}

/// A cover moss draws INTO the card, whole and uncropped.
///
/// The card is the only 1200x630 frame moss controls; a file handed to a
/// platform is cropped by that platform to a slot moss cannot see. So for a
/// cover whose shape no slot fits — a hanging scroll, a handscroll, a seal —
/// moss fits the entire picture inside the frame and sets the page title
/// beside it, the way a museum hangs a plate next to its label.
///
/// `width`/`height` are the SCANNED dimensions (`MediaMetadata.dimensions`),
/// not a probe: they size the layout, and they are part of the cache key so a
/// re-crop of the same filename re-renders. `len` and `mtime` complete that
/// key for an edit that keeps the dimensions.
#[derive(Debug, Clone, Copy)]
pub struct CardPlate<'a> {
    pub source: &'a Path,
    /// `source`'s path relative to the vault root — what `content_hash`
    /// keys on. `source` itself stays absolute (it's what gets opened to
    /// decode the image); hashing it directly baked the absolute path into
    /// the OG-card filename, so the same vault built from two different
    /// checkouts, or build_parity_test's two temp-dir copies of one
    /// fixture, got two different hashes for byte-identical output
    /// (found 2026-09-13 chasing a parity-test flake).
    pub relative_path: &'a str,
    pub width: u32,
    pub height: u32,
    pub len: u64,
    pub mtime_secs: i64,
}

/// Which regional glyph forms a card is set in. There are two; `Language::En`
/// is not one of them, which is why this type exists rather than passing a
/// `Language` down to the font chain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CardScript {
    Traditional,
    Simplified,
}

impl CardScript {
    /// The cache-key token. Two inputs that resolve to the same script render
    /// identical bytes, so they must hash to the same card.
    fn code(self) -> &'static str {
        match self {
            CardScript::Traditional => "hant",
            CardScript::Simplified => "hans",
        }
    }
}

/// The script a card's CJK glyphs should use, or `None` for "no Chinese
/// signal — leave the face to usvg".
///
/// A declared Chinese language is taken at its word. `En` is the ABSENCE of a
/// language signal, not a claim the title is Latin — a vault that never
/// declared one still has CJK titles — so instead of guessing we read the
/// title's own script with the same evidence-weighing judge the site language
/// uses (`i18n::detect::detect_language`, which short-circuits on CJK-dominant
/// text precisely so a short title still resolves).
///
/// `None` is not a fallback to Simplified, and that distinction is the whole
/// point: both chains list only Chinese faces, and a Chinese face covers kana
/// as well as kanji, so forcing one on an undeclared JAPANESE title would set
/// it in Chinese glyph forms and never hand the character back to usvg. The
/// bug this resolver exists to fix, inverted. No signal means no selector.
fn card_script(lang: crate::i18n::Language, title: &str) -> Option<CardScript> {
    let script_of = |l| match l {
        crate::i18n::Language::ZhHant => Some(CardScript::Traditional),
        crate::i18n::Language::ZhHans => Some(CardScript::Simplified),
        crate::i18n::Language::En => None,
    };
    script_of(lang).or_else(|| {
        // `detect_language` counts Han blocks and knows nothing of kana, so a
        // mostly-kanji Japanese title reads to it as CJK-dominant with no
        // Traditional evidence — i.e. Simplified. One kana is proof it is not
        // Chinese at all. Japanese written in kanji alone still misreads and
        // nothing here can fix it: `lang` arrives as moss's three-variant UI
        // language, so `[site] lang = "ja"` is already `En` by this point.
        //
        // U+30FB KATAKANA MIDDLE DOT `・` is excluded from the range: it is
        // punctuation, not kana, and Chinese authors use it between the parts
        // of a foreign name (尚-呂克・高達). Counting it as Japanese dropped
        // the selector and handed such a title to usvg's database order — the
        // Simplified-face-on-a-Traditional-title bug, back through a comma.
        let kana = |c: char| {
            matches!(c, '\u{3040}'..='\u{30fa}' | '\u{30fc}'..='\u{30ff}' | '\u{31f0}'..='\u{31ff}')
        };
        if title.chars().any(kana) {
            return None;
        }
        crate::i18n::detect::detect_language(title).and_then(script_of)
    })
}

/// The card sink handed to the page render, and the previous manifest it needs
/// to decide a reuse.
///
/// One value, not two parameters: "there is somewhere to record this card" and
/// "there is a manifest to look it up in" are the same fact, and splitting them
/// would let a caller supply a sink with no lookup — the state whose only exit
/// is a read of `.moss/build/`.
pub struct OgSink<'a> {
    previous_files: &'a std::collections::HashMap<String, String>,
    cards: Vec<CardOutput>,
}

impl<'a> OgSink<'a> {
    pub fn new(previous_files: &'a std::collections::HashMap<String, String>) -> Self {
        Self { previous_files, cards: Vec::new() }
    }

    /// Render (or carry) one card and record its receipt, returning the served
    /// path for the `og:image` tag.
    pub fn render(
        &mut self,
        inputs: &CardInputs,
        output_root: &Path,
    ) -> Result<&ServedPath, CardError> {
        let out = render_card(inputs, output_root, self.previous_files)?;
        self.cards.push(out);
        Ok(self.cards.last().expect("just pushed").served_path())
    }

    pub fn into_cards(self) -> Vec<CardOutput> {
        self.cards
    }
}

/// What a card call produced — a manifest receipt, never a promise the caller
/// has to go to disk to redeem.
///
/// The two arms are the only two registrable states. A card that is on disk but
/// absent from the previous manifest is neither: its hash exists nowhere in
/// memory, and reading it back is exactly the read this design removes. So the
/// reuse test ANDs the disk and the manifest (see [`render_card`]) and that
/// state is never constructed.
pub enum CardOutput {
    /// This call rasterized the card and wrote it. `bytes` are the PNG as
    /// written, held in memory so the caller can hash them.
    Rendered { served_path: ServedPath, bytes: Vec<u8> },
    /// The content-addressed card was already on disk with bytes AND the
    /// previous build has a manifest entry for it. Nothing was read; `entry` is
    /// that previous entry, to be carried forward verbatim.
    Carried { served_path: ServedPath, entry: String },
}

impl CardOutput {
    /// Record this card in the manifest. Both arms hold everything the entry
    /// needs — bytes on one, the previous entry on the other — so nothing here
    /// touches the disk.
    pub fn register(&self, pending: &mut crate::build::manifest::PendingManifest) {
        match self {
            CardOutput::Rendered { served_path, bytes } => {
                pending.register(served_path, bytes, crate::build::manifest::HashBucket::ImageOutputs)
            }
            CardOutput::Carried { served_path, entry } => {
                pending.register_hashed(served_path, entry, crate::build::manifest::HashBucket::ImageOutputs)
            }
        }
    }

    /// The served path, whichever arm this is. Disk location is
    /// `served_path().to_disk(output_root)`; URL is
    /// `served_path().to_absolute_url(&site_url)` or `to_relative_url()`.
    pub fn served_path(&self) -> &ServedPath {
        match self {
            CardOutput::Rendered { served_path, .. }
            | CardOutput::Carried { served_path, .. } => served_path,
        }
    }
}

/// Render the OG card to PNG and return its manifest receipt.
///
/// `previous_files` is the previous build's `files` map. A card already on disk
/// is reused only if that map has an entry for it: the on-disk copy carries no
/// hash anyone still holds, so without an entry to carry there would be nothing
/// to register but the bytes — and fetching those means reading `.moss/build/`,
/// which is the read this change exists to delete. Rasterizing instead is
/// deterministic and costs one render.
pub fn render_card(
    inputs: &CardInputs,
    output_root: &Path,
    previous_files: &std::collections::HashMap<String, String>,
) -> Result<CardOutput, CardError> {
    // Validate colors before any work. Hex strings are interpolated raw into
    // SVG attributes, so reject anything that could break out of the quote.
    for (name, value) in [
        ("bg_color", inputs.bg_color),
        ("fg_color", inputs.fg_color),
        ("accent_color", inputs.accent_color),
    ] {
        if !is_valid_hex_color(value) {
            return Err(CardError::Svg(format!("invalid color for {}: {}", name, value)));
        }
    }

    // Resolved ONCE and passed down: the cache key and the font chain must
    // describe the same card, and they are the same fact.
    let script = card_script(inputs.lang, inputs.title);
    let hash = content_hash(inputs, script);
    let served_path = ServedPath::for_og_card(&hash)
        .map_err(|e| CardError::Svg(format!("served path: {}", e)))?;
    let disk_path = served_path.to_disk(output_root);

    // Idempotency: reuse the on-disk card only when the previous manifest can
    // still name its hash.
    if output_present(&disk_path) {
        if let Some(entry) = previous_files.get(served_path.as_str()) {
            return Ok(CardOutput::Carried { served_path, entry: entry.clone() });
        }
    }

    // Decoded before the SVG is written, because the layout is computed from
    // the DECODED dimensions: scan's `(w, h)` keys the cache, but EXIF
    // orientation means the pixels can be the other way round, and the frame
    // the SVG strokes has to be the frame the picture lands in. A decode
    // failure is an error, not an empty frame — the caller falls back to
    // handing the platform the file itself, which is what it did before.
    let plate_img = inputs.plate.map(|p| decode_plate(p.source)).transpose()?;
    let rect = plate_img
        .as_ref()
        .map(|img| layout_plate(inputs, img.width(), img.height()));
    let svg = build_svg(inputs, rect);
    let mut pixmap = rasterize(&svg, 1200, 630, script)?;
    if let (Some(img), Some(rect)) = (plate_img.as_ref(), rect) {
        draw_plate(&mut pixmap, img, rect)?;
    }
    let bytes = pixmap.encode_png().map_err(|e| CardError::Encode(e.to_string()))?;

    // `write_output`, not a bare write: `output_root` is under `.moss/build/`,
    // and it lands the bytes in a uniquely-named temp sibling before renaming
    // onto the content-addressed path. The path is addressed by (script, title,
    // site_name, colors) — site_name and the colors are build constants,
    // so two pages with the SAME title collide on ONE path, and two workers in
    // the parallel page render (build/render/blocking.rs) would otherwise
    // interleave a non-atomic write there and tear the PNG. rename(2) is atomic
    // within a directory, so every reader sees either no file or a complete
    // one; identical inputs produce identical bytes, so whichever writer wins
    // is immaterial.
    if let Err(e) = crate::build::io_utils::write_output(&disk_path, &bytes) {
        // Windows refuses a replace while a peer's replace of the same
        // destination is in flight (ERROR_ACCESS_DENIED). Identical inputs
        // produce identical bytes, so a completed peer write IS this write.
        if !output_present(&disk_path) {
            return Err(CardError::Io(e));
        }
    }

    Ok(CardOutput::Rendered { served_path, bytes })
}

fn content_hash(inputs: &CardInputs, script: Option<CardScript>) -> String {
    let mut h = Sha256::new();
    // Renderer-version salt: cards are cached on disk by this hash alone
    // (the len>0 reuse in render_card), so any change to layout, fonts, or
    // build_svg MUST bump this version — otherwise cards rendered by an
    // older moss survive upgrades indefinitely.
    // v4: the key holds the resolved SCRIPT rather than the language, so an
    // undeclared page and a `zh-hans` one with the same title share the one
    // card they render identically — and an undeclared page with a Traditional
    // title no longer collides with the Simplified rendering it used to get.
    // `none` is its own key: a title with no Chinese signal renders through
    // usvg's own selector and is not the same bytes as either chain.
    // v5 (2026-09-11): the CJK chain leads with a SERIF face, so every CJK
    // card's pixels changed; and a page with a cover can now render a PLATE
    // card, a layout v4 had no key for.
    h.update(b"og-render-v5\0");
    h.update(script.map_or("none", CardScript::code).as_bytes());
    h.update(b"\0");
    h.update(inputs.title.as_bytes());
    h.update(b"\0");
    h.update(inputs.site_name.as_bytes());
    h.update(b"\0");
    h.update(inputs.bg_color.as_bytes());
    h.update(b"\0");
    h.update(inputs.fg_color.as_bytes());
    h.update(b"\0");
    h.update(inputs.accent_color.as_bytes());
    // Appended only for the vertical layout, so every horizontal card keeps
    // the key it had: v4 stays the horizontal layout's version, and no card
    // on a horizontal site is re-rendered for a layout it does not use.
    if inputs.vertical {
        h.update(b"\0vertical");
    }
    // The plate's BYTES are what changed, and hashing a 20 MB scan per card
    // would cost more than the render. Dimensions + length + mtime is the
    // same stat triple `extract_media_metadata_cached` keys its scan on, so a
    // cover the scan re-read is a cover this card re-renders.
    if let Some(plate) = inputs.plate {
        h.update(b"\0plate\0");
        h.update(plate.relative_path.as_bytes());
        h.update(plate.width.to_le_bytes());
        h.update(plate.height.to_le_bytes());
        h.update(plate.len.to_le_bytes());
        h.update(plate.mtime_secs.to_le_bytes());
    }
    let bytes = h.finalize();
    bytes.iter().take(8).map(|b| format!("{:02x}", b)).collect::<String>()
}

/// The card's geometry — title fitting, the four SVG layouts, and the plate
/// compositor. Split out 2026-09-11 when the plate card pushed this file past
/// its 800-line budget; the seam is "what a card looks like" against "when a
/// card is rendered, keyed, cached and written", which is what stayed here.
pub(super) mod layout;
use layout::{build_svg, decode_plate, draw_plate, layout_plate};

/// Process-wide fontdb: embedded Inter first (the named family — Latin
/// rendering stays pinned to the bundled face on every machine), then the
/// host's fonts, which usvg consults per MISSING glyph. That fallback is
/// what turns CJK titles from "NO GLYPH" tofu into real glyphs (PingFang
/// on macOS). WHICH host face serves those glyphs is chosen by
/// [`select_cjk_fallback`], not by whatever this database happens to list
/// first — see that function for why. CJK pixel output still varies by
/// host (a machine with no Chinese face at all cannot render one):
/// acceptable, because snapshot comparison excludes OG PNGs and the disk
/// cache keys on inputs + render version, not pixels. Built once:
/// load_system_fonts() scans font directories and render_card runs on
/// parallel workers.
///
/// Inter is OFL-licensed; see src/assets/fonts/Inter-LICENSE.txt. No CJK
/// face is bundled — they are megabytes, and the host always has one.
fn shared_fontdb() -> &'static std::sync::Arc<usvg::fontdb::Database> {
    static DB: std::sync::OnceLock<std::sync::Arc<usvg::fontdb::Database>> =
        std::sync::OnceLock::new();
    DB.get_or_init(|| {
        // Stored deflate-compressed (~1.2 MB of OTF per architecture raw);
        // inflated once, here, on first card render. Paths are relative to
        // this crate's manifest dir.
        include_flate::flate!(static INTER_REGULAR: [u8] from "src/assets/fonts/Inter-Regular.otf");
        include_flate::flate!(static INTER_BOLD: [u8] from "src/assets/fonts/Inter-Bold.otf");
        let mut db = usvg::fontdb::Database::new();
        db.load_font_data(INTER_REGULAR.to_vec());
        db.load_font_data(INTER_BOLD.to_vec());
        // Map generic family `sans-serif` to Inter so legacy SVG references
        // still resolve to the embedded font.
        db.set_sans_serif_family("Inter");
        db.load_system_fonts();
        std::sync::Arc::new(db)
    })
}

/// CJK faces we are willing to set a card in, most wanted first.
///
/// Two fully-ordered chains rather than one plus a suffix: a Traditional page
/// would rather have a Simplified face than none (the glyphs are mostly
/// shared), so each chain ends with the other script's faces. Neither chain
/// names a Japanese or Korean face — those cover Han and so render Chinese
/// without tofu, but in the wrong regional glyph forms, which is exactly the
/// bug this table exists to prevent. They remain reachable through usvg's own
/// selector as a last resort, so a host with only `Apple SD Gothic Neo` still
/// renders readable text instead of boxes.
///
/// Each chain lists macOS, then Linux, then Windows families.
/// The Chinese families we rank, each script listed ONCE.
///
/// A card is set in one of these two orders: the page's own script first, the
/// other script after it (better a Simplified face than a Korean one). Listing
/// each family once and ordering at the call site is what stops the two orders
/// drifting — as two hand-maintained full chains already had, one carrying
/// `Hiragino Sans CNS` and the other `Hiragino Sans GB`, so a page could fall
/// out of the chain entirely and back into the database-order selector this
/// exists to replace.
/// Serif (明體/宋體) leads each chain, because the card sets a TITLE and the
/// page sets its titles in `--moss-font-heading` — a serif stack in every
/// language moss ships (`site.css`, the `html[lang="zh-Hant"]` block names
/// `Source Han Serif TC, Noto Serif CJK TC, Songti TC, PMingLiU`). Until
/// 2026-09-11 the card led with PingFang/Heiti, so a page whose heading was
/// Song shared as a gothic slab: the card re-deciding something the page had
/// already decided, which is the defect the 2026-08-10 share-card audit named.
/// Sans faces stay behind the serifs so a host with no CJK serif still renders
/// glyphs rather than tofu.
const TRADITIONAL_FAMILIES: &[&str] = &[
    "Source Han Serif TC", "Noto Serif CJK TC", "Noto Serif TC", "Songti TC",
    "PMingLiU", "MingLiU",
    "PingFang TC", "PingFang HK", "Heiti TC", "Hiragino Sans CNS",
    "Noto Sans CJK TC", "Noto Sans CJK HK", "Noto Sans TC", "Noto Sans HK",
    "Source Han Sans TC",
    "Microsoft JhengHei",
];
const SIMPLIFIED_FAMILIES: &[&str] = &[
    "Source Han Serif SC", "Noto Serif CJK SC", "Noto Serif SC", "Songti SC",
    "STSong", "SimSun",
    "PingFang SC", "Heiti SC", "Hiragino Sans GB",
    "Noto Sans CJK SC", "Noto Sans SC", "Source Han Sans SC",
    "Microsoft YaHei", "SimHei", "WenQuanYi Zen Hei",
];

/// The glyph-fallback chain for `script`, own script first.
fn cjk_fallback_families(script: CardScript) -> impl Iterator<Item = &'static str> + Clone {
    let [first, second] = match script {
        CardScript::Traditional => [TRADITIONAL_FAMILIES, SIMPLIFIED_FAMILIES],
        CardScript::Simplified => [SIMPLIFIED_FAMILIES, TRADITIONAL_FAMILIES],
    };
    first.iter().chain(second.iter()).copied()
}

/// Pick the face that serves a glyph Inter is missing.
///
/// usvg 0.42 resolves `font-family` by NAME ONLY — `fontdb::Database::query`
/// returns the first family in the list that exists, with no glyph-coverage
/// test — so listing `Inter, PingFang TC, …` in the SVG would stop at Inter and
/// never reach the rest. Verified against the pinned usvg/fontdb source; a
/// family list in `build_svg` would be decoration, and it is deliberately not
/// there. Per-glyph fallback runs through this hook instead.
///
/// usvg's own selector returns the first face in DATABASE ORDER that covers the
/// character and is not already in use. Database order is the order the host's
/// font directories happened to be scanned in — it encodes no preference — and
/// the "not already in use" clause makes the answer depend on which faces an
/// earlier character in the same title already consumed. So the face varies
/// between characters, between runs of text, and between cards: a
/// Traditional-Chinese site got `Heiti TC` on one card and the KOREAN
/// `Apple SD Gothic Neo` on the next within one session (v0.11.6 user report).
/// Korean and Japanese faces cover Han, so nothing errors and nothing shows
/// tofu — the text is simply set in the wrong regional glyph forms.
///
/// Returning `None` here hands the character back to that selector, which is
/// the honest degradation for a host with no Chinese face at all.
fn select_cjk_fallback(
    c: char,
    used: &[usvg::fontdb::ID],
    db: &usvg::fontdb::Database,
    script: CardScript,
) -> Option<usvg::fontdb::ID> {
    let base = db.face(*used.first()?)?;
    let (weight, style, stretch) = (base.weight, base.style, base.stretch);

    for family in cjk_fallback_families(script) {
        // Query by name rather than scanning faces, so fontdb picks the right
        // WEIGHT within the family — the title is bold, and `Noto Sans CJK TC`
        // alone spans seven weights. Absent families simply return None.
        let Some(id) = db.query(&usvg::fontdb::Query {
            families: &[usvg::fontdb::Family::Name(family)],
            weight,
            style,
            stretch,
        }) else {
            continue;
        };
        // A face already in use cannot help (usvg is here because it produced a
        // missing glyph), and a subsetted family may not cover this character.
        if !used.contains(&id) && face_has_char(db, id, c) {
            return Some(id);
        }
    }
    None
}

/// Does `id` have a glyph for `c`?
///
/// fontdb 0.18 has no coverage query and usvg's own `DatabaseExt::has_char` is
/// crate-private, so parse the face — byte for byte what usvg does internally,
/// and only for the handful of families in the chain rather than every face on
/// the host.
fn face_has_char(db: &usvg::fontdb::Database, id: usvg::fontdb::ID, c: char) -> bool {
    db.with_face_data(id, |data, index| {
        ttf_parser::Face::parse(data, index)
            .ok()
            .and_then(|face| face.glyph_index(c))
            .is_some()
    }) == Some(true)
}

fn rasterize(
    svg: &str,
    width: u32,
    height: u32,
    script: Option<CardScript>,
) -> Result<tiny_skia::Pixmap, CardError> {
    let mut opt = usvg::Options::default();
    // Arc-share, don't deep-clone: fontdb_mut() is Arc::make_mut and would
    // copy every FaceInfo (hundreds of system faces) per card.
    opt.fontdb = std::sync::Arc::clone(shared_fontdb());
    if let Some(script) = script {
        let usvg_fallback = usvg::FontResolver::default_fallback_selector();
        opt.font_resolver.select_fallback = Box::new(move |c, used, db| {
            select_cjk_fallback(c, used, db, script).or_else(|| usvg_fallback(c, used, db))
        });
    }

    let tree = usvg::Tree::from_str(svg, &opt).map_err(|e| CardError::Svg(e.to_string()))?;
    let mut pixmap = tiny_skia::Pixmap::new(width, height)
        .ok_or_else(|| CardError::Encode("pixmap allocation failed".into()))?;
    resvg::render(&tree, tiny_skia::Transform::default(), &mut pixmap.as_mut());
    Ok(pixmap)
}

pub(super) fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[derive(Debug)]
pub enum CardError {
    Io(std::io::Error),
    Svg(String),
    /// The page's own picture would not decode. Its own arm because the caller
    /// acts on it: it hands the platform the file instead of a card, which is
    /// not what it does for a broken SVG.
    Plate(String),
    Encode(String),
}

impl std::fmt::Display for CardError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CardError::Io(e) => write!(f, "io: {}", e),
            CardError::Svg(e) => write!(f, "svg: {}", e),
            CardError::Plate(e) => write!(f, "plate: {}", e),
            CardError::Encode(e) => write!(f, "encode: {}", e),
        }
    }
}

impl std::error::Error for CardError {}

/// Re-register the previous build's OG cards when their pages were carried.
///
/// A page's card is written and registered by the same render call a carry
/// skips, and `remove_stale_files` deletes any `image_outputs` entry this
/// build didn't re-register — so without this the card of an unmodified page
/// would be pruned and its `og:image` would 404 until that page next
/// re-rendered (moss#966).
///
/// Cards are content-addressed by (title, site_name, colors) under
/// `_moss/og/`, and a carry only happens when no page's surface moved, so
/// every card the previous build produced is still the right card. The whole
/// prefix is carried rather than attributing cards to pages: over-keeping a
/// card costs a few KB until the next full build, under-keeping it breaks an
/// unmodified page's social preview.
///
/// Two facts are required per card, and both used to come from one read of
/// `.moss/build/`: the hash, and the proof the file is there. The manifest
/// supplies the hash; `output_present` is the other half — the same conjunct a
/// reuse in `render_card` applies. An entry without a file would put a path
/// in the manifest that no generation contains, and publish would refuse the
/// whole upload (`deploy.rs`, "Manifest claims '…' exists").
pub(crate) fn carry_previous_cards(
    previous: &crate::types::content::SiteHashes,
    output_dir: &Path,
    pending: &mut crate::build::manifest::PendingManifest,
) {
    for key in previous.image_outputs.iter() {
        // `from_source` rejects the reserved `_moss/` namespace, so the card
        // is rebuilt through its named constructor — which also re-validates
        // the hash shape, filtering the .webp variants sharing this bucket.
        let Some(hash) = key
            .strip_prefix(crate::build::served_path::OG_CARD_PREFIX)
            .and_then(|rest| rest.strip_suffix(".png"))
        else {
            continue;
        };
        let Ok(sp) = ServedPath::for_og_card(hash) else {
            continue;
        };
        let Some(entry) = previous.files.get(sp.as_str()) else {
            continue;
        };
        if !output_present(&sp.to_disk(output_dir)) {
            continue;
        }
        pending.register_hashed(&sp, entry, crate::build::manifest::HashBucket::ImageOutputs);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_previous() -> std::collections::HashMap<String, String> {
        std::collections::HashMap::new()
    }

    fn sample_inputs() -> CardInputs<'static> {
        CardInputs {
            title: "Hello world",
            site_name: "Example Site",
            bg_color: "#faf8f5",
            fg_color: "#1a1816",
            accent_color: "#d97706",
            lang: crate::i18n::Language::En,
            vertical: false,
            plate: None,
        }
    }

    fn vertical_inputs(title: &'static str) -> CardInputs<'static> {
        CardInputs { title, lang: crate::i18n::Language::ZhHant, vertical: true, ..sample_inputs() }
    }

    /// Pixels in `x0..x1 × y0..y1` that are not the sample background.
    fn ink(pixmap: &tiny_skia::Pixmap, x0: usize, x1: usize, y0: usize, y1: usize) -> usize {
        let data = pixmap.data();
        let bg = [0xfa_i32, 0xf8, 0xf5];
        let mut n = 0;
        for y in y0..y1 {
            for x in x0..x1 {
                let i = (y * 1200 + x) * 4;
                if (0..3).any(|c| (data[i + c] as i32 - bg[c]).abs() > 30) {
                    n += 1;
                }
            }
        }
        n
    }

    /// The horizontal layout is untouched by the vertical one: same SVG shape
    /// and, because the vertical token is appended only when set, the same
    /// cache key — so no card on a horizontal site is re-rendered.
    #[test]
    fn horizontal_cards_keep_their_layout_and_cache_key() {
        assert_eq!(hash_of(&sample_inputs()), "098b107c06c928de");
        let svg = build_svg(&sample_inputs(), None);
        assert!(!svg.contains("writing-mode"), "{svg}");
        assert!(svg.contains(r#"<text x="80" y="280""#), "{svg}");
        let mut v = sample_inputs();
        v.vertical = true;
        assert_ne!(hash_of(&v), hash_of(&sample_inputs()));
    }

    /// A vertical page's title card is ONE centred block — the title column
    /// with the site name's shorter column beside it — not a title hung on
    /// one edge of the card with the site name on the other.
    ///
    /// The right-hung form left the card's whole middle empty, which at the
    /// ~500px a timeline actually renders reads as a blank image with a word
    /// in the corner. The block still sits inside the centre 630x630 square
    /// WeChat crops to, because on a card with no picture the title IS the
    /// content.
    #[test]
    fn vertical_card_centres_its_title_block() {
        let inputs = vertical_inputs("\u{81f4}\u{65b9}\u{58eb}\u{743f}\u{66f8}");
        let svg = build_svg(&inputs, None);
        assert_eq!(svg.matches(r#"writing-mode="tb""#).count(), 2, "{svg}");
        let pixmap = rasterize(&svg, 1200, 630, card_script(inputs.lang, inputs.title)).expect("render");
        assert!(ink(&pixmap, 610, 690, 84, 560) > 2000, "no title column at x=650");
        assert!(ink(&pixmap, 530, 570, 84, 560) > 200, "no site-name column at x=550");
        // The block and nothing else: everything outside 500..700 is ground.
        assert_eq!(ink(&pixmap, 0, 500, 20, 630), 0, "ink left of the block");
        assert_eq!(ink(&pixmap, 700, 1200, 20, 630), 0, "ink right of the block");
        // A Latin title stacks too — sideways down the same column, as CSS
        // vertical-rl sets a Latin run on the page. Two columns here, so the
        // block is one pitch wider and still centred on x=600.
        let latin = vertical_inputs("Letter to Fang");
        let pixmap = rasterize(&build_svg(&latin, None), 1200, 630, None).expect("render");
        assert!(ink(&pixmap, 664, 736, 84, 560) > 2000, "Latin title not in the column");
        assert_eq!(ink(&pixmap, 0, 450, 20, 630), 0, "ink left of the two-column block");
        assert_eq!(ink(&pixmap, 745, 1200, 20, 630), 0, "ink right of the two-column block");
    }

    /// Write a solid-colour JPEG of the given size, and answer its path.
    fn plate_file(dir: &std::path::Path, name: &str, w: u32, h: u32) -> std::path::PathBuf {
        let path = dir.join(name);
        image::RgbImage::from_pixel(w, h, image::Rgb([20, 90, 200]))
            .save_with_format(&path, image::ImageFormat::Jpeg)
            .expect("write plate");
        path
    }

    fn plate_inputs<'a>(source: &'a std::path::Path, w: u32, h: u32, vertical: bool) -> CardInputs<'a> {
        CardInputs {
            title: "\u{96d9}\u{9df9}\u{5716}",
            lang: crate::i18n::Language::ZhHant,
            vertical,
            plate: Some(CardPlate {
                source,
                relative_path: source.to_str().unwrap_or("unused"),
                width: w,
                height: h,
                len: 0,
                mtime_secs: 0,
            }),
            ..sample_inputs()
        }
    }

    /// A hanging scroll is drawn WHOLE — the card never crops what a platform
    /// would have cropped for it.
    ///
    /// This is the invariant the whole plate layout exists for, so it is
    /// asserted on the geometry rather than on pixels: the rect's aspect ratio
    /// equals the picture's, and the rect is inside the card.
    #[test]
    fn a_plate_is_fitted_whole_never_cropped() {
        let tall = plate_inputs(std::path::Path::new("unused"), 2290, 4000, true);
        let r = layout_plate(&tall, 2290, 4000);
        assert!((r.w / r.h - 2290.0 / 4000.0).abs() < 0.01, "{r:?} is not the scroll's shape");
        assert!(r.x >= 80.0 && r.x + r.w <= 900.0 && r.y >= 50.0 && r.y + r.h <= 580.0, "{r:?}");
        // …and its centre stays inside the 630x630 square WeChat crops to.
        let cx = r.x + r.w / 2.0;
        assert!((285.0..=915.0).contains(&cx), "plate centre {cx} outside the WeChat square");

        let wide = plate_inputs(std::path::Path::new("unused"), 3400, 447, false);
        let r = layout_plate(&wide, 3400, 447);
        assert!((r.w / r.h - 3400.0 / 447.0).abs() < 0.05, "{r:?} is not the handscroll's shape");
        assert!(r.x >= 80.0 && r.x + r.w <= 1120.0, "{r:?}");

        // A picture smaller than its box is left small rather than blown up
        // past the point where it stops being the work.
        let small = plate_inputs(std::path::Path::new("unused"), 120, 120, false);
        let r = layout_plate(&small, 120, 120);
        assert_eq!((r.w, r.h), (240.0, 240.0), "a small square was upscaled past 2x");
    }

    /// The picture reaches the pixels, in the frame the layout named, with the
    /// title beside it rather than over it.
    #[test]
    fn a_plate_card_draws_the_picture_beside_the_title() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = plate_file(dir.path(), "scroll.jpg", 229, 400);
        let inputs = plate_inputs(&file, 229, 400, true);
        let out = render_card(&inputs, dir.path(), &no_previous()).expect("render");
        let CardOutput::Rendered { bytes, .. } = &out else { panic!("expected a render") };
        let card = image::load_from_memory(bytes).expect("decode card").to_rgb8();

        let rect = layout_plate(&inputs, 229, 400);
        let mid = card.get_pixel((rect.x + rect.w / 2.0) as u32, (rect.y + rect.h / 2.0) as u32);
        // JPEG is lossy; the assertion is "this is the plate", not "these
        // are the bytes".
        let near = |p: &image::Rgb<u8>, c: [u8; 3]| {
            (0..3).all(|i| (p.0[i] as i32 - c[i] as i32).abs() <= 8)
        };
        assert!(near(mid, [20, 90, 200]), "the plate's own colour is not in its frame: {mid:?}");
        // Outside the frame is the card's ground, not the picture.
        assert!(!near(card.get_pixel(150, 300), [20, 90, 200]), "the plate bled left of its frame");
        // And the title still stands in its column, clear of the picture.
        let pixmap = rasterize(&build_svg(&inputs, Some(rect)), 1200, 630, Some(CardScript::Traditional))
            .expect("render");
        assert!(ink(&pixmap, 1070, 1150, 64, 500) > 2000, "no title column at x=1110");

        // Same picture, no plate: a different card. Without this the whole
        // feature could be a no-op and every other assertion here would hold.
        let title_only = CardInputs { plate: None, ..inputs };
        let plain = render_card(&title_only, dir.path(), &no_previous()).expect("render");
        assert_ne!(plain.served_path().as_str(), out.served_path().as_str());
    }

    /// The card sets a title, and the page sets its titles in a serif. The
    /// card's chain must lead with a face the page's `--moss-font-heading`
    /// stack also names, or a Song heading shares as a gothic slab.
    #[test]
    fn the_cjk_chain_leads_with_a_face_the_page_also_asks_for() {
        let css = include_str!("../../assets/css/site.css");
        let block = css
            .split("html[lang=\"zh-Hant\"]")
            .nth(1)
            .expect("no zh-Hant block in site.css");
        let heading = block
            .split("--moss-font-heading:")
            .nth(1)
            .and_then(|rest| rest.split(';').next())
            .expect("no --moss-font-heading in the zh-Hant block");
        let lead = TRADITIONAL_FAMILIES[0];
        assert!(
            heading.contains(lead),
            "the card leads with {lead}, which the page's heading stack does not name: {heading}"
        );
    }

    #[test]
    fn overlong_vertical_title_takes_a_second_column_then_ellipsizes() {
        // 10 Han glyphs: over one 6.5em column, under two.
        let two = vertical_inputs("\u{7dda}\u{6027}\u{6ce8}\u{610f}\u{529b}\u{80cc}\u{5f8c}\u{7684}\u{8996}\u{89d2}");
        let svg = build_svg(&two, None);
        assert!(svg.contains(r#"<text x="700""#) && svg.contains(r#"<text x="600""#), "{svg}");
        assert!(!svg.contains('\u{2026}'));
        // 20: beyond two columns.
        let long = vertical_inputs("\u{7dda}\u{6027}\u{6ce8}\u{610f}\u{529b}\u{80cc}\u{5f8c}\u{7684}\u{8996}\u{89d2}\u{8f49}\u{63db}\u{8207}\u{91cf}\u{5b50}\u{529b}\u{5b78}\u{7684}\u{555f}\u{793a}");
        let svg = build_svg(&long, None);
        assert_eq!(svg.matches("<text x=\"700\"").count() + svg.matches("<text x=\"600\"").count(), 2);
        assert!(!svg.contains(r#"<text x="800""#), "{svg}");
        assert!(svg.contains('\u{2026}'));
    }

    /// The card's key for its own inputs — what `render_card` computes.
    fn hash_of(inputs: &CardInputs) -> String {
        content_hash(inputs, card_script(inputs.lang, inputs.title))
    }

    #[test]
    fn content_hash_changes_with_title() {
        let mut a = sample_inputs();
        let mut b = sample_inputs();
        b.title = "Different";
        assert_ne!(hash_of(&a), hash_of(&b));
        // Ensure a is read-after-write
        a.title = sample_inputs().title;
        assert_eq!(hash_of(&a), hash_of(&sample_inputs()));
    }

    /// The hash keys on the plate's path RELATIVE to the vault root, not its
    /// absolute form: two vaults holding the same picture at the same
    /// relative location must hash the same even when one is checked out
    /// (or copied, as build_parity_test does) somewhere else on disk.
    #[test]
    fn content_hash_ignores_the_plates_absolute_prefix() {
        let plate_a = CardPlate {
            source: std::path::Path::new("/tmp/checkout-a/images/cover.jpg"),
            relative_path: "images/cover.jpg",
            width: 2000,
            height: 800,
            len: 12345,
            mtime_secs: 1_700_000_000,
        };
        let plate_b = CardPlate {
            source: std::path::Path::new("/var/other/checkout-b/images/cover.jpg"),
            relative_path: "images/cover.jpg",
            ..plate_a
        };
        let mut same_relative = sample_inputs();
        same_relative.plate = Some(plate_a);
        let mut also_same_relative = sample_inputs();
        also_same_relative.plate = Some(plate_b);
        assert_eq!(
            hash_of(&same_relative),
            hash_of(&also_same_relative),
            "same vault-relative path, different absolute prefix, must hash the same"
        );

        let mut different_relative = sample_inputs();
        different_relative.plate =
            Some(CardPlate { relative_path: "images/other.jpg", ..plate_a });
        assert_ne!(
            hash_of(&same_relative),
            hash_of(&different_relative),
            "a genuinely different relative path must still change the hash"
        );
    }

    #[test]
    fn renders_png_with_correct_dimensions() {
        let dir = tempfile::tempdir().expect("tempdir");
        let out = render_card(&sample_inputs(), dir.path(), &no_previous()).expect("render");
        let disk = out.served_path().to_disk(dir.path());
        let bytes = std::fs::read(&disk).expect("read png");
        // PNG signature
        assert_eq!(&bytes[..8], &[137, 80, 78, 71, 13, 10, 26, 10]);
        // IHDR width (bytes 16..20) and height (20..24), big-endian
        let width = u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
        let height = u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
        assert_eq!(width, 1200);
        assert_eq!(height, 630);
    }

    #[test]
    fn url_path_uses_moss_framework_namespace() {
        let dir = tempfile::tempdir().expect("tempdir");
        let out = render_card(&sample_inputs(), dir.path(), &no_previous()).expect("render");
        let url = out.served_path().to_relative_url();
        assert!(
            url.starts_with("/_moss/og/"),
            "OG cards must live under /_moss/og/; got {}",
            url
        );
        assert!(url.ends_with(".png"));
    }

    /// The reuse gate is the disk AND the previous manifest, not the disk
    /// alone: a card whose entry is gone has no hash anyone still holds, and
    /// the only way to get one back without reading `.moss/build/` is to
    /// rasterize it again.
    #[test]
    fn reuse_needs_both_the_file_and_a_previous_entry() {
        let dir = tempfile::tempdir().expect("tempdir");
        let first = render_card(&sample_inputs(), dir.path(), &no_previous()).expect("first");
        let CardOutput::Rendered { served_path, .. } = &first else {
            panic!("first render into an empty dir must rasterize");
        };
        let disk = served_path.to_disk(dir.path());
        let mtime1 = std::fs::metadata(&disk).unwrap().modified().unwrap();

        // Entry present: carried verbatim, nothing rewritten.
        let previous = std::collections::HashMap::from([(
            served_path.as_str().to_string(),
            "100644:deadbeef".to_string(),
        )]);
        std::thread::sleep(std::time::Duration::from_millis(20));
        match render_card(&sample_inputs(), dir.path(), &previous).expect("second") {
            CardOutput::Carried { served_path: sp, entry } => {
                assert_eq!(sp.as_str(), served_path.as_str());
                assert_eq!(entry, "100644:deadbeef");
            }
            CardOutput::Rendered { .. } => panic!("expected the card to be carried"),
        }
        assert_eq!(mtime1, std::fs::metadata(&disk).unwrap().modified().unwrap());

        // Same file on disk, no entry: rasterized again, and the receipt
        // carries the bytes so the caller never has to read them back.
        match render_card(&sample_inputs(), dir.path(), &no_previous()).expect("third") {
            CardOutput::Rendered { bytes, .. } => {
                assert_eq!(&bytes[..8], &[137, 80, 78, 71, 13, 10, 26, 10]);
                assert_eq!(bytes, std::fs::read(&disk).expect("read png"));
            }
            CardOutput::Carried { .. } => panic!("no previous entry: must rasterize"),
        }
    }

    #[test]
    fn xml_escape_handles_specials() {
        assert_eq!(xml_escape("a & b < c > d"), "a &amp; b &lt; c &gt; d");
        assert_eq!(xml_escape(r#"say "hi""#), r#"say &quot;hi&quot;"#);
    }

    #[test]
    fn truncates_long_title_with_ellipsis() {
        // Use a distinctive char ('Z') not present in the SVG template.
        let long_title: String = "Z".repeat(200);
        let mut inputs = sample_inputs();
        inputs.title = &long_title;

        // Width-aware layout: an unbroken 200-char run fills two lines and
        // ellipsizes. ASCII estimates at 0.5em → ≤ 2 × 28 chars survive.
        let svg = build_svg(&inputs, None);
        assert!(svg.contains('\u{2026}'), "expected ellipsis in truncated SVG");
        let z_run = svg.chars().filter(|c| *c == 'Z').count();
        assert!(
            z_run <= 58 && z_run >= 40,
            "expected ~2 lines of Z (40..=58), got {}",
            z_run
        );

        // End-to-end render must still produce a valid PNG without panicking.
        let dir = tempfile::tempdir().expect("tempdir");
        let out = render_card(&inputs, dir.path(), &no_previous()).expect("render");
        let bytes = std::fs::read(&out.served_path().to_disk(dir.path())).expect("read png");
        assert_eq!(&bytes[..8], &[137, 80, 78, 71, 13, 10, 26, 10]);
    }

    #[test]
    fn cjk_title_wraps_to_a_second_line_by_width() {
        // 20 full-width chars ≈ 20em — over one 14.4em line, under two.
        // The old 40-CHAR cap would have crammed all 20 onto one overflowing
        // line; width-aware layout must split without an ellipsis.
        let mut inputs = sample_inputs();
        inputs.title = "线性注意力背后的视角转换与量子力学的启示";
        let svg = build_svg(&inputs, None);
        let title_tspans = svg.matches(r#"<tspan x="80""#).count();
        assert_eq!(title_tspans, 2, "expected 2 title lines: {}", svg);
        assert!(!svg.contains('\u{2026}'), "20 CJK chars fit 2 lines, no ellipsis");
    }

    #[test]
    fn overlong_cjk_title_ellipsizes_on_the_second_line() {
        // 34 full-width chars ≈ 34em — beyond two 14.4em lines.
        let mut inputs = sample_inputs();
        inputs.title = "这是一个特别特别特别特别特别特别特别特别特别特别特别特别特别特别特别长的标题";
        let svg = build_svg(&inputs, None);
        let title_tspans = svg.matches(r#"<tspan x="80""#).count();
        assert_eq!(title_tspans, 2, "title never exceeds 2 lines");
        assert!(svg.contains('\u{2026}'), "overlong CJK title must ellipsize");
    }

    #[test]
    fn latin_title_prefers_space_breaks() {
        // A long Latin title should break at a word gap, not mid-word.
        let mut inputs = sample_inputs();
        inputs.title = "The Long History of Automating the Things We Love Doing Every Day";
        let svg = build_svg(&inputs, None);
        // No line may end or begin mid-word: check no tspan content ends
        // with a letter directly followed by a tspan starting mid-word is
        // hard to assert generically — pin the concrete split instead:
        // 66 chars ≈ 33em → 2 lines; the break must land on a space.
        let title_tspans = svg.matches(r#"<tspan x="80""#).count();
        assert_eq!(title_tspans, 2);
        // Extract tspan contents
        let mut contents: Vec<&str> = Vec::new();
        let mut rest = svg.as_str();
        while let Some(start) = rest.find(r#"<tspan x="80""#) {
            let after = &rest[start..];
            let open = after.find('>').unwrap();
            let close = after.find("</tspan>").unwrap();
            contents.push(&after[open + 1..close]);
            rest = &after[close + 8..];
        }
        for c in &contents {
            assert!(!c.starts_with(' ') && !c.ends_with(' '), "line has stray space: {:?}", c);
        }
        // Words survive intact: every whitespace-split token of the original
        // that appears must appear whole in one of the lines.
        let joined = contents.join(" ");
        for word in ["History", "Automating", "Things"] {
            assert!(joined.contains(word), "word {} broken across lines: {:?}", word, contents);
        }
    }

    #[test]
    fn long_site_name_truncates_to_one_line() {
        let long_name = "特别".repeat(40);
        let mut inputs = sample_inputs();
        inputs.site_name = &long_name;
        let svg = build_svg(&inputs, None);
        // The site-name tspan must be ellipsized, never wrapped.
        let name_count = svg.matches("特别").count();
        assert!(name_count < 40, "site name should truncate, kept {}", name_count);
        assert!(svg.contains('\u{2026}'));
    }

    #[test]
    fn inter_resolves_to_the_embedded_face() {
        // The commit's central invariant: the named "Inter" family must
        // resolve to the EMBEDDED binary faces, never to a system-installed
        // Inter — that is what keeps Latin rendering identical across
        // machines while system fonts ride along for CJK glyph fallback.
        // Host-independent: fontdb 0.18 query() walks faces in insertion
        // order, and the embedded faces load first.
        use usvg::fontdb::{Family, Query, Source, Weight};
        for weight in [Weight::NORMAL, Weight::BOLD] {
            let id = shared_fontdb()
                .query(&Query {
                    families: &[Family::Name("Inter")],
                    weight,
                    ..Default::default()
                })
                .expect("family 'Inter' must resolve");
            let (source, _) = shared_fontdb().face_source(id).expect("face source");
            assert!(
                matches!(source, Source::Binary(_)),
                "'Inter' at weight {:?} resolved to a system file, not the \
                 embedded face — Latin rendering is no longer machine-stable",
                weight
            );
        }
    }

    #[test]
    fn chinese_is_ranked_by_script_not_by_font_database_order() {
        use crate::i18n::Language;
        // `select_cjk_fallback` walks the chain in order and takes the first
        // family the host has, so the chain's own order IS the decision. The
        // reported failure was a Traditional site set in the KOREAN
        // `Apple SD Gothic Neo` because it happened to come first in the host's
        // font database; ranking by script is what makes that unreachable.
        let rank = |script, family: &str| {
            cjk_fallback_families(script).position(|f| f == family)
        };

        // Traditional first for a Traditional page, Simplified for a Simplified
        // one — and each still prefers the other script to nothing.
        let (hant, hans) = (CardScript::Traditional, CardScript::Simplified);
        assert!(rank(hant, "Heiti TC") < rank(hant, "PingFang SC"));
        assert!(rank(hans, "PingFang SC") < rank(hans, "Heiti TC"));
        assert!(rank(hant, "PingFang SC").is_some());
        assert!(rank(hans, "PingFang TC").is_some());

        // Each family is listed once, so the two orders are permutations of ONE
        // set — a family cannot be added to one chain and forgotten in the other.
        let mut a: Vec<_> = cjk_fallback_families(hant).collect();
        let mut b: Vec<_> = cjk_fallback_families(hans).collect();
        a.sort_unstable();
        b.sort_unstable();
        assert_eq!(a, b, "the two chains must offer the same families");
        assert_eq!(a.len(), {
            let mut u = a.clone();
            u.dedup();
            u.len()
        }, "no family may appear twice in a chain");

        // A Korean or Japanese face is never ranked at all — usvg reaches one
        // only after the whole chain misses.
        assert_eq!(rank(hant, "Apple SD Gothic Neo"), None);
        assert_eq!(rank(hans, "Hiragino Sans"), None);
        let _ = Language::En;
    }

    #[test]
    fn every_ranked_family_is_a_chinese_one() {
        use crate::i18n::Language;
        // A Japanese or Korean family added to either chain would reintroduce
        // the bug silently, because it renders Han without tofu.
        const NOT_CHINESE: &[&str] = &[
            "Apple SD Gothic Neo", "AppleGothic", "Nanum Gothic", "Malgun Gothic",
            "Noto Sans CJK KR", "Noto Sans KR", "Source Han Sans KR",
            "Hiragino Sans", "Hiragino Kaku Gothic ProN", "Yu Gothic", "Meiryo",
            "Noto Sans CJK JP", "Noto Sans JP", "Source Han Sans JP", "MS Gothic",
        ];
        let _ = Language::En;
        for script in [CardScript::Simplified, CardScript::Traditional] {
            for family in cjk_fallback_families(script) {
                assert!(
                    !NOT_CHINESE.contains(&family),
                    "{:?} chain ranks the non-Chinese face {}",
                    script, family
                );
            }
        }
    }

    #[test]
    fn the_chosen_cjk_face_is_stable_and_script_correct_on_this_host() {
        use usvg::fontdb::{Family, Query, Weight};
        // Runs against the REAL host font database, which is where the drift
        // happened. Skipped on a host with no Chinese face — there the honest
        // outcome is usvg's own choice, asserted by the unit tests above.
        let db = shared_fontdb();
        let inter = |weight| {
            db.query(&Query { families: &[Family::Name("Inter")], weight, ..Default::default() })
                .expect("Inter must resolve")
        };
        let title_base = inter(Weight::BOLD);
        let name_base = inter(Weight::NORMAL);

        let has_family = |name: &str| {
            db.faces().any(|f| f.families.iter().any(|(n, _)| n == name))
        };
        if !cjk_fallback_families(CardScript::Traditional).any(|f| has_family(f)) {
            // Loud, not silent: a runner with no Chinese font reports this test
            // as `ok` having asserted nothing, which is the "green at the wrong
            // layer" failure this test exists to avoid. CI installs
            // fonts-noto-cjk (.github/workflows/pr.yml) so this should not fire
            // there.
            eprintln!("SKIPPED: no Chinese font on this host; the CJK face choice is unasserted");
            return;
        }

        let family_of = |id: usvg::fontdb::ID| -> String {
            db.face(id).expect("face").families[0].0.clone()
        };
        let pick = |base, script| {
            select_cjk_fallback('\u{7dda}', &[base], db, script).map(family_of)
        };

        // The answer is the same for the bold title and the regular site name —
        // usvg's selector, which excludes faces already in use, guarantees that
        // for neither.
        let title = pick(title_base, CardScript::Traditional).expect("a Chinese face is available");
        assert_eq!(pick(name_base, CardScript::Traditional).as_deref(), Some(title.as_str()));

        // The face is one we ranked, not whatever the database listed first.
        assert!(
            cjk_fallback_families(CardScript::Traditional).any(|f| f == title),
            "resolved to unranked family {}",
            title
        );

        // Where the host offers both scripts, the two languages must not land
        // in the same face — that is the whole point of ranking per language.
        let hans = pick(title_base, CardScript::Simplified).expect("a Chinese face is available");
        let has_hant = ["PingFang TC", "Noto Sans CJK TC", "Heiti TC"].iter().any(|f| has_family(f));
        let has_hans = ["PingFang SC", "Noto Sans CJK SC", "Heiti SC"].iter().any(|f| has_family(f));
        if has_hant && has_hans {
            assert_ne!(
                title, hans,
                "Traditional and Simplified pages resolved to the same face on a \
                 host that offers both scripts"
            );
        }
    }

    #[test]
    fn the_rendered_card_changes_with_the_script_not_just_the_chain() {
        // The only test whose assertion can FAIL if `rasterize` stops
        // installing `opt.font_resolver.select_fallback` — where the fix lives.
        // Others rasterize, but with Latin titles that never reach the hook,
        // and the rest call `select_cjk_fallback`/`cjk_fallback_families`, so
        // deleting the resolver assignment — the obvious casualty of hoisting
        // `usvg::Options` into the fontdb's `OnceLock` — leaves them all green
        // while every CJK card silently returns to database-order faces.
        //
        // Two scripts, one title: the same glyphs set in Traditional-first and
        // Simplified-first chains must produce different pixels. Gated on the
        // host actually having both, because on a one-script host the correct
        // answer IS the same face.
        let db = shared_fontdb();
        let has_family = |name: &str| db.faces().any(|f| f.families.iter().any(|(n, _)| n == name));
        let has_hant = TRADITIONAL_FAMILIES.iter().any(|f| has_family(f));
        let has_hans = SIMPLIFIED_FAMILIES.iter().any(|f| has_family(f));
        if !(has_hant && has_hans) {
            eprintln!("SKIPPED: host lacks a Traditional or Simplified face; the resolver is unasserted");
            return;
        }

        let mut inputs = sample_inputs();
        // Traditional and Simplified share these code points, so the glyphs
        // differ only by the FACE chosen — exactly what the resolver decides.
        inputs.title = "\u{7dda}\u{689d}\u{8207}\u{7d50}\u{69cb}";
        let svg = build_svg(&inputs, None);
        let hant = rasterize(&svg, 1200, 630, Some(CardScript::Traditional)).expect("rasterize hant");
        let hans = rasterize(&svg, 1200, 630, Some(CardScript::Simplified)).expect("rasterize hans");
        assert_ne!(
            hant.data(),
            hans.data(),
            "the same CJK title rendered in both chains produced identical pixels — \
             the font resolver is not reaching the rasterizer"
        );
    }

    #[test]
    fn an_undeclared_page_takes_its_script_from_the_title() {
        use crate::i18n::Language;
        // `En` is the absence of a signal. A Traditional title on a vault that
        // never declared a language used to be set in Simplified faces — the
        // reported bug in a milder form.
        assert_eq!(
            card_script(Language::En, "\u{7dda}\u{689d}\u{8207}\u{7d50}\u{69cb}\u{7684}\u{95dc}\u{4fc2}"),
            Some(CardScript::Traditional)
        );
        assert_eq!(card_script(Language::En, "\u{7ebf}\u{6761}\u{4e0e}\u{7ed3}\u{6784}\u{7684}\u{5173}\u{7cfb}"), Some(CardScript::Simplified));
        // A declared language is taken at its word, whatever the title says.
        assert_eq!(card_script(Language::ZhHant, "\u{7ebf}\u{6761}"), Some(CardScript::Traditional));
        // No Chinese evidence at all: no selector. A Japanese title lands here
        // too, and must — every family in both chains is a Chinese face that
        // also covers kana, so a guess here would silently set Japanese prose
        // in Chinese glyph forms instead of deferring to usvg.
        assert_eq!(card_script(Language::En, "Hello world"), None);
        assert_eq!(card_script(Language::En, "\u{65e5}\u{672c}\u{8a9e}\u{306e}\u{8a18}\u{4e8b}"), None);
        // 愛麗絲・夢遊仙境 — a Traditional title whose only non-Han character
        // is U+30FB, which Chinese authors use between the parts of a foreign
        // name. It must still select Traditional: the middle dot is
        // punctuation, and counting it as kana returned `None`, dropping the
        // selector and re-opening the very bug this resolver exists to close.
        assert_eq!(
            card_script(Language::En, "\u{611b}\u{9e97}\u{7d72}\u{30fb}\u{5922}\u{904a}\u{4ed9}\u{5883}"),
            Some(CardScript::Traditional)
        );
    }

    #[test]
    fn language_is_part_of_the_cache_key() {
        // Two languages render the same title in different faces, so they must
        // not share one content-addressed PNG.
        let mut hant = sample_inputs();
        hant.lang = crate::i18n::Language::ZhHant;
        let mut hans = sample_inputs();
        hans.lang = crate::i18n::Language::ZhHans;
        assert_ne!(hash_of(&hant), hash_of(&hans));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn cjk_titles_render_distinct_glyphs_not_tofu() {
        // With only embedded Inter, every missing CJK glyph rasterizes as the
        // SAME tofu box, so two same-length CJK titles produce near-identical
        // title regions. With system-font fallback (PingFang on macOS) the
        // glyphs differ. Assert the two renders diverge substantially.
        let mut a = sample_inputs();
        a.title = "线性注意力背后的视角转换";
        let mut b = sample_inputs();
        b.title = "纽约禅堂坐禅笔记与随想录";

        let pa = rasterize(&build_svg(&a, None), 1200, 630, card_script(a.lang, a.title)).expect("render a");
        let pb = rasterize(&build_svg(&b, None), 1200, 630, card_script(b.lang, b.title)).expect("render b");
        let (da, db) = (pa.data(), pb.data());
        let mut differing = 0usize;
        for y in 210..300 {
            for x in 80..1000 {
                let i = (y * 1200 + x) * 4;
                if da[i..i + 3] != db[i..i + 3] {
                    differing += 1;
                }
            }
        }
        assert!(
            differing > 5000,
            "two different CJK titles render nearly identically ({} differing \
             pixels) — glyphs are falling back to tofu boxes, not a real CJK face",
            differing
        );
    }

    #[test]
    fn rejects_invalid_bg_color() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut inputs = sample_inputs();
        inputs.bg_color = "red";
        let result = render_card(&inputs, dir.path(), &no_previous());
        match result {
            Err(CardError::Svg(_)) => {}
            Err(other) => panic!("expected CardError::Svg, got {:?}", other),
            Ok(_) => panic!("expected CardError::Svg, got Ok"),
        }
    }

    #[test]
    fn rasterize_is_deterministic_across_runs() {
        // Determinism check within one process: rendering the same inputs
        // twice produces the exact same pixmap bytes. (Cross-machine Latin
        // determinism is guarded by inter_resolves_to_the_embedded_face —
        // this test cannot see which face rendered.)
        let svg = build_svg(&sample_inputs(), None);
        let p1 = rasterize(&svg, 1200, 630, None).expect("render 1");
        let p2 = rasterize(&svg, 1200, 630, None).expect("render 2");
        assert_eq!(
            p1.data(),
            p2.data(),
            "rasterize must be deterministic; bytes differ between runs",
        );
    }

    #[test]
    fn latin_title_renders_from_embedded_inter() {
        // Dense pixel scan of the title region: some sans face must have
        // rendered ink. (WHICH face is asserted by
        // inter_resolves_to_the_embedded_face — with system fonts loaded,
        // non-blankness alone can no longer prove the embedded font was
        // used.)
        //
        // Strategy: scan every pixel in the title bbox, count those that
        // differ from the background. Glyph anti-aliased edges + interior
        // dark pixels give thousands of differing pixels under normal
        // rendering. A bare background gives zero. Threshold loosely so
        // we're robust to font-version pixel shifts.
        let svg = build_svg(&sample_inputs(), None);
        let pixmap = rasterize(&svg, 1200, 630, None).expect("render");
        let data = pixmap.data();
        let bg = [0xfa, 0xf8, 0xf5];
        let mut differing = 0usize;
        for y in 220..300 {
            for x in 80..1080 {
                let i = ((y * 1200 + x) * 4) as usize;
                let r = data[i] as i32;
                let g = data[i + 1] as i32;
                let b = data[i + 2] as i32;
                let dr = (r - bg[0]).abs();
                let dg = (g - bg[1]).abs();
                let db = (b - bg[2]).abs();
                if dr > 30 || dg > 30 || db > 30 {
                    differing += 1;
                }
            }
        }
        // 80×1000 = 80,000 pixels in the scan; a rendered title typically
        // gives thousands of differing pixels. Require at least 500.
        assert!(
            differing >= 500,
            "embedded font appears to have been dropped: only {} of 80,000 \
             title-region pixels differ from background",
            differing
        );
    }

    #[test]
    fn concurrent_renders_of_same_card_never_tear() {
        // Regression for the parallel page render (build/render/blocking.rs):
        // the OG card disk path is content-addressed by (title, site_name,
        // colors) ONLY, and site_name + colors are build constants — so two
        // pages with the SAME title map to ONE path. Under the parallel render
        // multiple workers call render_card on that shared path at once and
        // then read it back to hash for the manifest. A non-atomic save_png
        // tears the PNG and lets a reader observe partial bytes (→ corrupt
        // shipped card + manifest/disk hash mismatch, ADR-013). render_card
        // must write atomically (temp + rename) so every reader sees a
        // complete, identical file regardless of interleaving.
        use std::sync::{Arc, Barrier};

        for iter in 0..4 {
            let dir = tempfile::tempdir().expect("tempdir");
            let root = dir.path().to_path_buf();
            const N: usize = 16;
            let barrier = Arc::new(Barrier::new(N));
            let handles: Vec<_> = (0..N)
                .map(|_| {
                    let root = root.clone();
                    let barrier = Arc::clone(&barrier);
                    std::thread::spawn(move || {
                        // Identical inputs on every thread → identical content
                        // hash → the SAME disk path (the colliding-title case).
                        let inputs = sample_inputs();
                        barrier.wait(); // release together to maximize write/read overlap
                        let out = render_card(&inputs, &root, &no_previous()).expect("render");
                        let disk = out.served_path().to_disk(&root);
                        std::fs::read(&disk).expect("read back")
                    })
                })
                .collect();
            let results: Vec<Vec<u8>> =
                handles.into_iter().map(|h| h.join().expect("join")).collect();

            // Every read-back must be a complete, valid 1200×630 PNG.
            for (i, bytes) in results.iter().enumerate() {
                assert!(
                    bytes.len() >= 24,
                    "iter {iter} read-back {i} is truncated ({} bytes) — torn write",
                    bytes.len()
                );
                assert_eq!(
                    &bytes[..8],
                    &[137, 80, 78, 71, 13, 10, 26, 10],
                    "iter {iter} read-back {i} has an invalid PNG signature — torn write"
                );
                let width = u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
                let height = u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
                assert_eq!((width, height), (1200, 630), "iter {iter} read-back {i} has a torn header");
            }
            // Identical inputs are deterministic, so every read-back must be
            // byte-identical — a differing snapshot means a torn/racy write.
            let first = &results[0];
            for (i, bytes) in results.iter().enumerate() {
                assert_eq!(bytes, first, "iter {iter} read-back {i} differs — racy write");
            }
            // No temp file may be left behind after the atomic rename.
            let og_dir = root.join("_moss").join("og");
            let leftover: Vec<String> = std::fs::read_dir(&og_dir)
                .expect("read og dir")
                .filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|n| n.contains("pending"))
                .collect();
            assert!(leftover.is_empty(), "iter {iter} leftover temp files: {leftover:?}");
        }
    }

    /// Both halves of the carry are load-bearing: the manifest supplies the
    /// hash, the disk supplies the proof there is a file to serve. A card
    /// registered without bytes would name a path no generation contains, and
    /// publish refuses the whole upload on that.
    #[test]
    fn carry_keeps_the_card_that_is_on_disk_and_drops_the_one_that_is_not() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let present = ServedPath::for_og_card("abc1234567890def").expect("sp");
        let missing = ServedPath::for_og_card("0123456789abcdef").expect("sp");
        let empty = ServedPath::for_og_card("fedcba9876543210").expect("sp");

        std::fs::create_dir_all(root.join("_moss").join("og")).expect("mkdir");
        std::fs::write(present.to_disk(root), b"\x89PNG-ish").expect("write");
        std::fs::write(empty.to_disk(root), b"").expect("write");

        let mut previous = crate::types::content::SiteHashes::default();
        for sp in [&present, &missing, &empty] {
            previous.image_outputs.insert(sp.as_str().to_string());
            previous
                .files
                .insert(sp.as_str().to_string(), crate::types::content::file_entry("deadbeef"));
        }
        // A .webp variant shares this bucket and is not a card.
        previous.image_outputs.insert("photos/x.webp".to_string());

        let mut pending = crate::build::manifest::PendingManifest::new(previous.clone());
        carry_previous_cards(&previous, root, &mut pending);
        let sealed = pending.seal();

        assert!(
            sealed.image_outputs().contains(present.as_str()),
            "the card with bytes on disk must carry: {:?}",
            sealed.image_outputs()
        );
        for gone in [&missing, &empty] {
            assert!(
                !sealed.image_outputs().contains(gone.as_str()),
                "{} has no bytes on disk and must not be registered",
                gone.as_str()
            );
        }
        assert!(
            !sealed.image_outputs().contains("photos/x.webp"),
            "the carry must not launder a non-card out of this bucket"
        );
    }
}
