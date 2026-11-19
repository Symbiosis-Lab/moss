//! What a site's theme decides about its generated link-preview card.
//!
//! Read once per build from `.moss/theme/`: the colours (`--moss-share-card-bg`,
//! `--moss-share-card-ink`, `--moss-share-card-muted`), the heading face the theme hosts,
//! and a picture for the card's inline end (`--moss-share-card-picture`). Every part
//! is optional; a theme that sets none of them gets the card moss always drew.

use crate::build::cli_output::log_note;
use crate::build::emit::theme_css_assets::{normalize, read_theme_file};
use crate::build::emit::theme_font_weights::{hosted_faces, unquoted};
use crate::build::theme_lint::{strip_comments, Layered};
use sha2::{Digest, Sha256};
use std::path::Path;
use std::sync::Arc;

/// The widest the picture may be: 45% of the 1200px card.
const MAX_PICTURE_WIDTH: u32 = 540;
/// The card's height, which the picture fills.
const CARD_HEIGHT: u32 = 630;
#[derive(Default)]
pub struct CardTheme {
    pub bg: Option<String>,
    pub ink: Option<String>,
    pub muted: Option<String>,
    pub heading: Option<HeadingFace>,
    pub picture: Option<CardPicture>,
}

impl std::fmt::Debug for CardTheme {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CardTheme")
            .field("bg", &self.bg)
            .field("heading", &self.heading.as_ref().map(|h| &h.family))
            .field("picture", &self.picture.is_some())
            .finish_non_exhaustive()
    }
}

/// A TrueType or OpenType face the theme hosts for its headings, loaded over
/// the host's fonts so the card can name it.
pub struct HeadingFace {
    pub family: String,
    pub digest: [u8; 32],
    pub(super) db: Arc<usvg::fontdb::Database>,
}

/// The theme's picture, cropped to the shape of the strip it fills.
pub struct CardPicture {
    pub image: image::DynamicImage,
    pub digest: [u8; 32],
}

impl CardPicture {
    /// The strip's width: the picture's aspect at card height, capped.
    pub fn width(&self) -> u32 {
        let (w, h) = (f64::from(self.image.width()), f64::from(self.image.height().max(1)));
        ((f64::from(CARD_HEIGHT) * w / h).round() as u32).clamp(1, MAX_PICTURE_WIDTH)
    }
}

impl CardTheme {
    /// [`CardTheme::of_theme`] over the site's `.moss/theme/` folder.
    pub fn of_site(css: Option<&str>, source_root: &Path, evidence: &crate::build::cloud_ledger::InputEvidence) -> Arc<Self> {
        let theme_dir = crate::moss_paths::MossPaths::new(source_root).theme_dir();
        let real_theme = theme_dir.canonicalize().ok();
        Arc::new(Self::of_theme(css, &mut |rel| read_theme_file(&theme_dir, real_theme.as_deref(), rel, evidence)))
    }

    /// Reads the theme's card tokens from `css`, and the files they name
    /// through `read`, which takes a path inside the theme folder.
    pub fn of_theme(css: Option<&str>, read: &mut dyn FnMut(&str) -> Option<Vec<u8>>) -> Self {
        let Some(css) = css else { return Self::default() };
        let layered = Layered::new(Some(css));
        let colour = |token: &str| {
            let (light, _) = layered.colours(token);
            light.root.map(|(r, g, b)| format!("#{r:02x}{g:02x}{b:02x}"))
        };
        let stripped = strip_comments(css);
        Self {
            bg: colour("--moss-share-card-bg"),
            ink: colour("--moss-share-card-ink"),
            muted: colour("--moss-share-card-muted"),
            heading: layered
                .values("--moss-font-heading")
                .0
                .and_then(|stack| heading_face(&stripped, &stack, read)),
            picture: layered
                .values("--moss-share-card-picture")
                .0
                .and_then(|value| first_url(&value))
                .and_then(|url| picture(&url, read)),
        }
    }

    /// What the theme changes in the card's pixels beyond its colours, for the
    /// card's cache key; each part is tagged, so equal bytes in two roles differ.
    pub fn key_parts(&self) -> impl Iterator<Item = (&'static [u8], &[u8])> {
        [
            (&b"muted"[..], self.muted.as_deref().map(str::as_bytes)),
            (&b"heading"[..], self.heading.as_ref().map(|h| &h.digest[..])),
            (&b"picture"[..], self.picture.as_ref().map(|p| &p.digest[..])),
        ]
        .into_iter()
        .filter_map(|(tag, part)| Some((tag, part?)))
    }
}

fn heading_face(stripped: &str, stack: &str, read: &mut dyn FnMut(&str) -> Option<Vec<u8>>) -> Option<HeadingFace> {
    let lead = unquoted(stack.split(',').next()?);
    let mut faces: Vec<_> = hosted_faces(stripped).into_iter().filter(|f| f.family.eq_ignore_ascii_case(&lead)).collect();
    // The title is set bold, so the face nearest 700 leads.
    faces.sort_by_key(|f| 700u16.clamp(f.lightest, f.heaviest).abs_diff(700));
    let wanted = faces.first().and_then(|f| font_files(&f.src).into_iter().next());
    for face in faces {
        for rel in font_files(&face.src) {
            let Some(bytes) = theme_rel(&rel).and_then(|rel| read(&rel)) else { continue };
            if let Some(face) = load_face(bytes) {
                return Some(face);
            }
        }
    }
    if let Some(file) = wanted {
        log_note!("Link previews use Inter, not \"{lead}\": add .moss/theme/{file} beside its web font");
    }
    None
}

/// The files a `src` list names that a card can load, in the order written: a
/// `.ttf` or `.otf` as itself, and for a `.woff` or `.woff2` the file of the
/// same name beside it with either extension. usvg's font loader reads neither
/// WOFF flavour.
fn font_files(src: &str) -> Vec<String> {
    let mut out = Vec::new();
    for url in urls(src) {
        let path = url.split(['?', '#']).next().unwrap_or_default();
        let Some((stem, ext)) = path.rsplit_once('.') else { continue };
        match ext.to_ascii_lowercase().as_str() {
            "ttf" | "otf" => out.push(path.to_string()),
            "woff" | "woff2" => out.extend([format!("{stem}.ttf"), format!("{stem}.otf")]),
            _ => {}
        }
    }
    out
}

fn load_face(bytes: Vec<u8>) -> Option<HeadingFace> {
    let digest: [u8; 32] = Sha256::digest(&bytes).into();
    let mut db = (**super::shared_fontdb()).clone();
    let loaded = db.load_font_source(usvg::fontdb::Source::Binary(Arc::new(bytes)));
    let family = db.face(*loaded.first()?)?.families.first()?.0.clone();
    // A host copy of the same family would be matched by name as well, and
    // the card would differ from host to host.
    let others: Vec<_> = db
        .faces()
        .filter(|f| !loaded.contains(&f.id) && f.families.iter().any(|(name, _)| name.eq_ignore_ascii_case(&family)))
        .map(|f| f.id)
        .collect();
    for id in others {
        db.remove_face(id);
    }
    Some(HeadingFace { family, digest, db: Arc::new(db) })
}

fn picture(url: &str, read: &mut dyn FnMut(&str) -> Option<Vec<u8>>) -> Option<CardPicture> {
    let bytes = read(&theme_rel(url)?)?;
    let digest: [u8; 32] = Sha256::digest(&bytes).into();
    let image = image::load_from_memory(&bytes).ok()?;
    Some(CardPicture { image: strip(image), digest })
}

/// The picture as the strip shows it, at the strip's own pixel size: whole when
/// its aspect at card height fits the cap, else its middle. Scaled once here,
/// so the build holds a card-sized picture and each card copies it.
fn strip(image: image::DynamicImage) -> image::DynamicImage {
    let (w, h) = (image.width().max(1), image.height().max(1));
    let kept = if CARD_HEIGHT * w <= MAX_PICTURE_WIDTH * h {
        w
    } else {
        (f64::from(h) * f64::from(MAX_PICTURE_WIDTH) / f64::from(CARD_HEIGHT)).round() as u32
    };
    let cropped = if kept == w { image } else { image.crop_imm((w - kept) / 2, 0, kept, h) };
    if h <= CARD_HEIGHT {
        return cropped;
    }
    let width = ((f64::from(kept) * f64::from(CARD_HEIGHT) / f64::from(h)).round() as u32).clamp(1, MAX_PICTURE_WIDTH);
    crate::build::media::scale::scale_exact(&cropped, width, CARD_HEIGHT)
}

/// A reference as a path inside the theme folder; none for an address or one that climbs out.
fn theme_rel(rel: &str) -> Option<String> {
    (!rel.contains(':') && !rel.starts_with('/')).then(|| normalize(rel)).flatten()
}

/// Every `url(...)` argument in `value`, unquoted.
fn urls(value: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = value;
    while let Some(at) = rest.to_ascii_lowercase().find("url(") {
        let after = rest.get(at + 4..).unwrap_or_default();
        let Some(end) = after.find(')') else { break };
        out.push(unquoted(after.get(..end).unwrap_or_default()));
        rest = after.get(end + 1..).unwrap_or_default();
    }
    out
}

fn first_url(value: &str) -> Option<String> {
    urls(value).into_iter().next()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build::page::og_card::CardInputs;
    use super::super::layout::theme_strip;

    fn svg_of(inputs: &CardInputs) -> String {
        super::super::build_svg(inputs, None)
    }

    fn key_of(inputs: &CardInputs) -> String {
        super::super::content_hash(inputs, None)
    }

    /// Inter with its family renamed, so a card can only name it from the theme.
    fn renamed_inter() -> Vec<u8> {
        let mut bytes = crate::build::page::og_card::inter_faces()[1].to_vec();
        let (old, new) = ("Inter", "Intro");
        let wide = |s: &str| s.encode_utf16().flat_map(u16::to_be_bytes).collect::<Vec<u8>>();
        for (from, to) in [(old.as_bytes().to_vec(), new.as_bytes().to_vec()), (wide(old), wide(new))] {
            let mut i = 0;
            while i + from.len() <= bytes.len() {
                if bytes[i..i + from.len()] == from[..] {
                    bytes[i..i + from.len()].copy_from_slice(&to);
                    i += from.len();
                } else {
                    i += 1;
                }
            }
        }
        bytes
    }

    const FACE_CSS: &str = "@font-face { font-family: \"Intro\"; font-weight: 700; src: url(\"fonts/intro.woff2\") format(\"woff2\"); }\n\
        :root { --moss-font-heading: \"Intro\", serif; }";

    fn theme_with(css: &str, files: &[(&str, Vec<u8>)]) -> CardTheme {
        CardTheme::of_theme(Some(css), &mut |p| files.iter().find(|(rel, _)| *rel == p).map(|(_, b)| b.clone()))
    }

    fn inputs(theme: CardTheme) -> CardInputs<'static> {
        CardInputs {
            title: "Garden notes", site_name: "Garden", bg_color: "#101820", fg_color: "#f0f0f0",
            accent_color: "#8a7a6f", lang: crate::i18n::Language::En, vertical: false, plate: None,
            theme: Some(Arc::new(theme)),
        }
    }

    #[test]
    fn the_heading_face_comes_from_a_ttf_beside_the_woff2() {
        let theme = theme_with(FACE_CSS, &[("fonts/intro.ttf", renamed_inter())]);
        let face = theme.heading.as_ref().expect("the ttf beside the woff2 is loaded");
        assert_eq!(face.family, "Intro");
        assert!(face.db.query(&usvg::fontdb::Query {
            families: &[usvg::fontdb::Family::Name("Intro")],
            ..Default::default()
        }).is_some());
        let svg = svg_of(&inputs(theme));
        assert!(svg.contains(r#"font-family="Intro""#) && !svg.contains(r#"font-family="Inter""#), "{svg}");
    }

    #[test]
    fn without_a_ttf_or_otf_the_card_keeps_inter() {
        crate::build::cli_output::take_cli_problems();
        let theme = theme_with(FACE_CSS, &[("fonts/intro.woff2", vec![0; 64])]);
        assert!(theme.heading.is_none());
        // The card still draws, in Inter: advice, not a problem `--strict` fails on.
        assert_eq!(crate::build::cli_output::take_cli_problems(), 0);
        assert!(svg_of(&inputs(theme)).contains(r#"font-family="Inter""#));
        let garbage = theme_with(FACE_CSS, &[("fonts/intro.ttf", vec![1; 64])]);
        assert!(garbage.heading.is_none(), "bytes that are not a font are not a face");
    }

    #[test]
    fn the_card_tokens_are_read_as_hex_and_a_theme_without_them_sets_nothing() {
        let theme = theme_with(":root { --moss-share-card-bg: rgb(16, 24, 32); --moss-share-card-muted: #aabbcc; }", &[]);
        assert_eq!((theme.bg.as_deref(), theme.ink.as_deref(), theme.muted.as_deref()), (Some("#101820"), None, Some("#aabbcc")));
        let none = theme_with(":root { --moss-color-bg: #fff; }", &[]);
        assert!(none.bg.is_none() && none.muted.is_none() && none.heading.is_none() && none.picture.is_none());
        let svg = svg_of(&inputs(theme));
        assert!(svg.contains(r##"fill="#aabbcc""##) && !svg.contains("opacity=\"0.65\""), "{svg}");
    }

    fn jpeg(w: u32, h: u32, shade: u8) -> Vec<u8> {
        let mut out = std::io::Cursor::new(Vec::new());
        image::RgbImage::from_pixel(w, h, image::Rgb([shade, 90, 200]))
            .write_to(&mut out, image::ImageFormat::Jpeg)
            .unwrap();
        out.into_inner()
    }

    const PICTURE_CSS: &str = ":root { --moss-share-card-picture: url(\"card-strip.jpg\"); }";

    #[test]
    fn the_picture_sits_at_the_inline_end_at_its_own_aspect_and_takes_the_spine_place() {
        let theme = theme_with(PICTURE_CSS, &[("card-strip.jpg", jpeg(300, 630, 20))]);
        let i = inputs(theme);
        let (picture, rect) = theme_strip(&i).expect("a strip");
        assert_eq!((picture.width(), rect.x, rect.w, rect.h), (300, 900.0, 300.0, 630.0));
        let svg = svg_of(&i);
        assert!(!svg.contains(r##"fill="#8a7a6f""##), "no spine beside a picture: {svg}");
        // Wider than the cap: 45% of the card, cropped about its middle.
        let wide = inputs(theme_with(PICTURE_CSS, &[("card-strip.jpg", jpeg(2000, 630, 20))]));
        let (picture, rect) = theme_strip(&wide).unwrap();
        assert_eq!((picture.width(), rect.x, picture.image.width()), (540, 660.0, 540));
    }

    #[test]
    fn a_large_picture_is_held_at_the_strips_size() {
        for ((w, h), held) in [((1000, 2100), (300, 630)), ((4000, 1260), (540, 630))] {
            let theme = theme_with(PICTURE_CSS, &[("card-strip.jpg", jpeg(w, h, 20))]);
            let image = &theme.picture.as_ref().expect("a picture").image;
            assert_eq!((image.width(), image.height()), held, "{w}x{h}");
        }
    }

    #[test]
    fn the_picture_is_part_of_the_cache_key_and_a_bare_theme_is_not() {
        let key = |shade: u8| key_of(&inputs(theme_with(PICTURE_CSS, &[("card-strip.jpg", jpeg(300, 630, shade))])));
        assert_ne!(key(20), key(120));
        assert_eq!(key(20), key(20));
        let bare = inputs(CardTheme::default());
        let mut plain = inputs(CardTheme::default());
        plain.theme = None;
        assert_eq!(key_of(&bare), key_of(&plain));
        assert_eq!(svg_of(&bare), svg_of(&plain));
    }

    #[test]
    fn one_file_as_heading_face_or_as_picture_keys_two_cards() {
        let digest = [7; 32];
        let face = HeadingFace { family: "Intro".into(), digest, db: Arc::clone(super::super::shared_fontdb()) };
        let picture = CardPicture { image: image::DynamicImage::new_rgb8(1, 1), digest };
        let heading_only = inputs(CardTheme { heading: Some(face), ..CardTheme::default() });
        let picture_only = inputs(CardTheme { picture: Some(picture), ..CardTheme::default() });
        assert_ne!(key_of(&heading_only), key_of(&picture_only));
    }
}
