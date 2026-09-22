//! Turning a [`Placement`] into markup: the three attributes every embed
//! kind wears, and the `<figure>` that carries them when there is a caption.
//!
//! One builder, so `data-width` means the same thing on a `<video>`, an
//! `<object>`, a `<model-viewer>` and a folder listing. Before this existed
//! three of them built the attribute byte-for-byte identically and two
//! silently dropped it.
//!
//! **The invariant both halves exist to protect:** `data-width`, the align
//! class AND the size style all sit on the OUTERMOST emitted element — the
//! figure when a caption is present, the element itself when not. The
//! content-width escape is `article.container > [data-width]`, a
//! direct-child selector, and the 50% float cap is keyed the same way; an
//! attribute one level deeper than the wrapper reaches neither rule. A
//! figure has no size of its own to fall back on, so when the size sits one
//! level deeper it stops applying to the figure at all — CSS fills the
//! inner element to 100% of the figure once the figure carries it instead.

use crate::media::Placement;
use crate::resolve::embed_renderer::html_escape_attr;

/// The markup fragments a [`Placement`] contributes to one element.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PlacementAttrs {
    /// ` data-width="wide"` (leading space), or empty.
    pub data_width_attr: String,
    /// The float class, to be appended to the element's own class list.
    pub align_class: Option<&'static str>,
    /// ` style="width:40%"` (leading space), or empty.
    pub size_style_attr: String,
}

impl PlacementAttrs {
    /// Append the float class to a base class list, e.g.
    /// `"moss-embed"` → `"moss-embed moss-align-right"`.
    pub fn class_value(&self, base: &str) -> String {
        match self.align_class {
            Some(c) => format!("{} {}", base, c),
            None => base.to_string(),
        }
    }

    /// The align class as a leading-space suffix — `" moss-align-left"` /
    /// `" moss-align-right"` / `""` — for a call site that spells its own
    /// class list literally (`class="moss-embed moss-embed-audio{}"`) rather
    /// than going through [`Self::class_value`]. `&'static str`, not a
    /// `format!`-built `String`: four call sites used to hand-roll this
    /// match themselves.
    pub fn align_suffix(&self) -> &'static str {
        match self.align_class {
            Some("moss-align-left") => " moss-align-left",
            Some("moss-align-right") => " moss-align-right",
            Some(_) | None => "",
        }
    }
}

/// Build the attribute fragments for one element.
pub fn placement_attrs(placement: &Placement) -> PlacementAttrs {
    PlacementAttrs {
        data_width_attr: match placement.width {
            Some(w) => format!(r#" data-width="{}""#, html_escape_attr(w)),
            None => String::new(),
        },
        align_class: placement.align.map(|side| side.css_class()),
        size_style_attr: match placement.size {
            Some(ref pct) => format!(r#" style="width:{}""#, html_escape_attr(pct)),
            None => String::new(),
        },
    }
}

/// The class moss puts on the `<figure>` wrapping a captioned non-image
/// embed. Parallel to `.moss-image` for images, and distinct from
/// `.moss-embed`, which stays on the functional element carrying
/// `data-type`.
///
/// Written as a literal everywhere it is emitted: the contract-table sync
/// test scans for `class="moss-…"` string literals, so a `format!`-assembled
/// class would pass the gate while the table drifted.
pub const CLASS_EMBED_FIGURE: &str = "moss-embed-figure";

/// Wrap an embed's HTML in a `<figure>` carrying its caption.
///
/// The whole placement moves out to this figure — width, align AND size —
/// so the caller passes the inner element `Placement::default()`: the inner
/// element wears none of it, the direct-child width escape keeps firing on
/// the figure, and CSS fills the inner element to 100% of whatever width
/// the figure ends up with. A figure with align and size but no width
/// otherwise sizes itself off its content, and a `<video>` or `<object>`
/// has none, so the size has to be readable from the outermost element too.
pub fn wrap_embed_with_caption(html: &str, placement: &Placement, caption: &str) -> String {
    let attrs = placement_attrs(placement);
    let caption = crate::media::html_escape(caption);
    // Both arms spell the class out: the contract-table sync test scans for
    // `class="moss-…"` literals and cannot see one that `format!` assembles.
    match attrs.align_class {
        Some(align) => format!(
            r#"<figure class="moss-embed-figure {align}"{width}{size}>{html}<figcaption>{caption}</figcaption></figure>"#,
            align = align,
            width = attrs.data_width_attr,
            size = attrs.size_style_attr,
            html = html,
            caption = caption,
        ),
        None => format!(
            r#"<figure class="moss-embed-figure"{width}{size}>{html}<figcaption>{caption}</figcaption></figure>"#,
            width = attrs.data_width_attr,
            size = attrs.size_style_attr,
            html = html,
            caption = caption,
        ),
    }
}

#[cfg(test)]
#[path = "placement_tests.rs"]
mod tests;
