//! The width / align / size vocabulary every embed kind shares.
//!
//! Lives beside the rest of the media vocabulary but in its own file,
//! because it is the one part of it that is NOT image-specific: a video, a
//! pdf, a 3D model and a folder listing all read the same three values off
//! the same pipe segments. Fit and object-position stay in the parent module
//! — they describe how a bitmap fills its box, which only an image has.

use super::{match_width_token, parse_image_width, AlignSide, Fit, Position};

/// Where an embed sits and how much room it takes: the named width escape
/// (`body | wide | page | screen`), the float side, and the float's own
/// width as a percent of the column.
///
/// Every embed kind — image, video, pdf, audio, iframe, 3D model, folder
/// listing — reads the same vocabulary out of the same pipe segments, so
/// this is the single carrier for all three. `ParsedEmbed` holds one of
/// these instead of a separate `width` field, which is why there is never a
/// second width to reconcile against.
///
/// Fit and object-position stay out: they describe how a bitmap fills its
/// box, which only an image has.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Placement {
    /// Named width escape — `body`, `wide`, `page`, `screen` (`full` is
    /// canonicalised to `screen`). Emitted as `data-width=`.
    pub width: Option<&'static str>,
    /// Float side for editorial runaround.
    pub align: Option<AlignSide>,
    /// The element's own width as a percent of the column, e.g. `"40%"` or
    /// `"50.5%"`. Accepts everything [`parse_image_width`] accepts, its
    /// tolerance of a space before the sign (`"55 %"`) included.
    pub size: Option<String>,
}

impl Placement {
    /// True when the author wrote no placement vocabulary at all.
    pub fn is_empty(&self) -> bool {
        self.width.is_none() && self.align.is_none() && self.size.is_none()
    }

    /// Fill only the fields still unset from `other`. Earlier segments win,
    /// so `align-right|33%|caption` keeps both, and a second width later in
    /// the pipe cannot silently replace the first.
    pub(crate) fn fill_from(&mut self, other: Placement) {
        if self.width.is_none() {
            self.width = other.width;
        }
        if self.align.is_none() {
            self.align = other.align;
        }
        if self.size.is_none() {
            self.size = other.size;
        }
    }
}

/// The align spellings that mean "float this", as opposed to
/// [`AlignSide::from_keyword`]'s fuller set.
///
/// Bare `left` / `right` are deliberately absent: in a pipe segment they
/// mean `object-position`, and have since before alignment existed
/// (`![[hero.jpg|left]]` positions the crop, it does not float the image).
/// [`AlignSide::from_keyword`] still accepts them because Stage 1 writes
/// them as the value of an explicit `align=` key, where nothing else can
/// claim them.
fn placement_align_token(tok: &str) -> Option<AlignSide> {
    match tok.to_lowercase().as_str() {
        "align-left" | "alignleft" => Some(AlignSide::Left),
        "align-right" | "alignright" => Some(AlignSide::Right),
        _ => None,
    }
}

/// Whether one whitespace-separated token is placement vocabulary: a named
/// width, a float keyword, or a percent.
fn is_placement_token(tok: &str) -> bool {
    match_width_token(tok).is_some()
        || placement_align_token(tok).is_some()
        || parse_image_width(tok).is_some_and(|w| w.ends_with('%'))
}

/// Whether a `|`-delimited segment may have placement pulled out of it.
///
/// True when every token is either placement vocabulary or an image display
/// keyword (fit / object-position, two-word positions included). The display
/// keywords are admitted so `align-right cover` and `wide cover` keep
/// working — they are one segment mixing both vocabularies, and only the
/// placement half is extracted. Prose is rejected whole, which is what keeps
/// a caption like `The page was left open` a caption.
fn is_placement_segment(segment: &str) -> bool {
    let tokens: Vec<&str> = segment.split_whitespace().collect();
    if tokens.is_empty() {
        return false;
    }
    let mut i = 0;
    while i < tokens.len() {
        if i + 1 < tokens.len() {
            let combined = format!("{} {}", tokens[i], tokens[i + 1]);
            if Position::from_keyword(&combined).is_some() {
                i += 2;
                continue;
            }
        }
        if is_placement_token(tokens[i])
            || Fit::from_keyword(tokens[i]).is_some()
            || Position::from_keyword(tokens[i]).is_some()
        {
            i += 1;
            continue;
        }
        return false;
    }
    true
}

/// Read a segment that is placement vocabulary in its entirety — the same
/// whole-input rule [`parse_image_width`] and [`extract_width_from_alias`]
/// already use, so `55 %` stays one value rather than two tokens.
fn whole_segment_placement(segment: &str) -> Option<Placement> {
    if let Some(w) = match_width_token(segment) {
        return Some(Placement {
            width: Some(w),
            ..Default::default()
        });
    }
    if let Some(side) = placement_align_token(segment) {
        return Some(Placement {
            align: Some(side),
            ..Default::default()
        });
    }
    match parse_image_width(segment) {
        Some(pct) if pct.ends_with('%') => Some(Placement {
            size: Some(pct),
            ..Default::default()
        }),
        _ => None,
    }
}

/// Pull the placement tokens out of one segment, returning them and whatever
/// tokens were left behind. `None` when the segment is not placement-shaped
/// at all, in which case the caller keeps it verbatim.
///
/// First match per field wins inside the segment.
pub(crate) fn parse_placement_segment(segment: &str) -> Option<(Placement, String)> {
    if let Some(p) = whole_segment_placement(segment) {
        return Some((p, String::new()));
    }
    if !is_placement_segment(segment) {
        return None;
    }
    let mut placement = Placement::default();
    let mut rest: Vec<&str> = Vec::new();
    let tokens: Vec<&str> = segment.split_whitespace().collect();
    let mut i = 0;
    while i < tokens.len() {
        // A two-word position is one value and belongs to the remainder whole.
        if i + 1 < tokens.len() {
            let combined = format!("{} {}", tokens[i], tokens[i + 1]);
            if Position::from_keyword(&combined).is_some() {
                rest.push(tokens[i]);
                rest.push(tokens[i + 1]);
                i += 2;
                continue;
            }
        }
        let tok = tokens[i];
        i += 1;
        if let Some(w) = match_width_token(tok) {
            if placement.width.is_none() {
                placement.width = Some(w);
                continue;
            }
        } else if let Some(side) = placement_align_token(tok) {
            if placement.align.is_none() {
                placement.align = Some(side);
                continue;
            }
        } else if let Some(pct) = parse_image_width(tok).filter(|p| p.ends_with('%')) {
            if placement.size.is_none() {
                placement.size = Some(pct);
                continue;
            }
        }
        rest.push(tok);
    }
    Some((placement, rest.join(" ")))
}

/// Walk every `|`-delimited segment of `alias`, folding the placement out of
/// each one and rejoining the rest.
///
/// Every placement-shaped segment contributes; the walk never stops at the
/// first. Later segments fill only the fields still unset, so
/// `align-right|33%|A caption` keeps the float, the size AND the caption —
/// a first-match-wins extraction would drop two of the three.
///
/// A whole segment is matched before its individual tokens are, which is
/// what keeps `55 %` a single percent instead of the two unparseable tokens
/// `55` and `%`.
pub fn extract_placement_from_alias(alias: &str) -> (Placement, String) {
    let mut placement = Placement::default();
    let mut remainder: Vec<String> = Vec::new();
    for seg in alias.split('|') {
        match parse_placement_segment(seg.trim()) {
            Some((found, rest)) => {
                placement.fill_from(found);
                if !rest.is_empty() {
                    remainder.push(rest);
                }
            }
            None => remainder.push(seg.to_string()),
        }
    }
    (placement, remainder.join("|"))
}
