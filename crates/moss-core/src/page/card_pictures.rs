//! The pictures a page's own body gives its listing card: the cover it takes
//! when it has no `cover:`, and the strip a `summary` row draws after it. The
//! build and a host's preview both read them from the parsed page here.

use crate::ast::{self, Document, Url};

/// A raster picture in the page body, as its resolved address and its alt text.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct BodyPicture {
    pub href: String,
    pub alt: String,
}

/// What a parsed page's own pictures are, its opening hero already lifted out.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PagePictures {
    /// The opening hero's picture.
    pub hero: Option<String>,
    /// The body's first picture, when it is a raster file.
    pub first: Option<String>,
    /// Every raster body picture, in reading order.
    pub body: Vec<BodyPicture>,
}

impl PagePictures {
    /// The pictures of `doc`, whose opening hero, if any, showed `hero`. The
    /// hero must already be out of `doc`, or its overlay's picture would be
    /// counted twice.
    pub fn of(doc: &Document, hero: Option<String>) -> Self {
        let images = ast::block_images(doc);
        let raster = |url: &Url| match url {
            Url::Resolved(r) if !r.href.ends_with('/') && is_raster_picture(&r.href) => Some(r.href.clone()),
            _ => None,
        };
        let first = images.first().and_then(|i| raster(i.src));
        let body = images.iter().filter_map(|i| Some(BodyPicture { href: raster(i.src)?, alt: i.alt.to_string() })).collect();
        Self { hero, first, body }
    }

    /// The listing card's picture when the page has no `cover:`: the hero's,
    /// else the body's first; none when `cover:` is `false`.
    pub fn card_cover(&self, cover_field: Option<&str>) -> Option<String> {
        if cover_field.is_some_and(|c| c.trim() == "false") {
            return None;
        }
        card_cover_from_page(self.hero.as_deref(), self.first.as_deref())
    }
}

/// Whether a path names a raster picture. The share image and the listing card
/// both take only these: most share targets cannot show a vector, and a card
/// crops a picture, not a drawing or a video.
pub fn is_raster_picture(path: &str) -> bool {
    let bare = path.split(['#', '?']).next().unwrap_or("");
    let ext = bare.rsplit('/').next().and_then(|f| f.rsplit_once('.')).map_or("", |(_, e)| e);
    matches!(ext.to_ascii_lowercase().as_str(), "png" | "jpg" | "jpeg" | "gif" | "webp" | "avif")
}

/// A page's own first picture as a listing-card source path, or `None`.
/// Remote, `data:`, vector and video pictures make poor cards, so they are not taken.
pub fn card_cover_from_page(hero_image_url: Option<&str>, body_cover_path: Option<&str>) -> Option<String> {
    [hero_image_url, body_cover_path].into_iter().flatten().find_map(card_picture_path)
}

/// One picture reference as a listing-card source path, or `None` when a card
/// cannot use it ([`card_cover_from_page`]).
fn card_picture_path(raw: &str) -> Option<String> {
    let (path, _attrs) = crate::media::split_pipe(raw);
    let local = !(path.starts_with("http://") || path.starts_with("https://") || path.starts_with("//") || path.starts_with("data:"));
    (local && is_raster_picture(path)).then(|| crate::resolve::fuzzy_path::percent_decode_path(path).trim_start_matches('/').to_string())
}

/// The pictures after the listing cover that a strip of `n` draws, in reading
/// order: the hero picture, then the body's raster pictures, none repeating the
/// cover or each other. Empty when the page has no listing cover (a page that
/// opted out with `cover: false` stays out).
pub fn card_pictures_after_cover(listing_cover: Option<&str>, hero_image_url: Option<&str>, body_images: &[BodyPicture], n: usize) -> Vec<String> {
    let Some(cover) = listing_cover else { return Vec::new() };
    let mut taken = vec![card_picture_path(cover).unwrap_or_else(|| cover.to_string())];
    for path in hero_image_url.into_iter().chain(body_images.iter().map(|p| p.href.as_str())).filter_map(card_picture_path) {
        if taken.len() >= n {
            break;
        }
        if !taken.contains(&path) {
            taken.push(path);
        }
    }
    taken.split_off(1)
}

#[cfg(test)]
#[path = "card_pictures_tests.rs"]
mod tests;
