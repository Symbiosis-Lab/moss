//! `content_width: screen`: the page's top-level grids and fenced-div bands
//! default to the `screen` width, so the author writes `:::grid 2 5:7` and
//! gets the edge-to-edge band without `{screen}` on every block.
//!
//! The page's width is only known after the folder cascade has run, which is
//! after the body is rendered and cached, so this works on the finished body
//! fragment. It adds `data-width="screen"`, the attribute `{screen}` writes, and
//! leaves every other width rule to the stylesheet's one `screen` path. A block
//! with a width word of its own (`{body}` included, which renders
//! `data-width="body"`) already carries the attribute and is left alone.
//!
//! A picture was given a `sizes` for the default content column, so the same
//! pass restates it: for the block a `screen` page widened around it, else for
//! the column a `wide`, `full` or `screen` page draws it in.

/// The `content_width` value that sets this default; the page's column is then
/// the `full` one.
pub(crate) const SCREEN: &str = "screen";

/// What `<body data-content-width>` says for a page's `content_width`: the
/// default only changes the blocks, so the column is the `full` column.
pub(crate) fn column_preset(content_width: &str) -> &str {
    if content_width == SCREEN { "full" } else { content_width }
}

/// `body` as a page of this resolved `content_width` draws it.
pub(crate) fn apply(body: String, content_width: Option<&str>) -> String {
    match content_width {
        Some(width @ ("wide" | "full" | SCREEN)) => widened(&body, width),
        _ => body,
    }
}

/// `body` with every picture's `sizes` restated for the page's column. On a
/// `screen` page, `data-width="screen"` is first added to every top-level grid
/// and fenced-div band that has no width of its own, and the pictures inside
/// are restated for that width instead. The selectors mirror the stylesheet's
/// `article.container > …` escape rules, so only a block the escape reaches is
/// touched.
fn widened(body: &str, content_width: &str) -> String {
    const OPEN: &str = r#"<article class="container">"#;
    const CLOSE: &str = "</article>";
    // The blocks the default widens. Selectors match the source's attributes,
    // so the pictures inside are still found once the block carries its width.
    const BLOCKS: [&str; 2] = [
        "article.container > .moss-grid:not([data-width])",
        r#"article.container > div[class]:not([data-width]):not([class^="moss-"]):not([class*=" moss-"])"#,
    ];
    use moss_core::contract::sizes::{sizes_in_page_column, sizes_in_screen_block};
    let restate = |el: &mut lol_html::html_content::Element, restated: &dyn Fn(&str) -> Option<String>| {
        match el.get_attribute("sizes").and_then(|sizes| restated(&sizes)) {
            Some(sizes) => el.set_attribute("sizes", &sizes),
            None => Ok(()),
        }
    };
    let mut handlers = Vec::new();
    if content_width == SCREEN {
        let pictures = BLOCKS.map(|block| format!("{block} img[sizes], {block} source[sizes]")).join(", ");
        handlers.push(lol_html::element!(BLOCKS.join(", "), |el| Ok(el.set_attribute("data-width", "screen")?)));
        handlers.push(lol_html::element!(pictures, |el| Ok(restate(el, &sizes_in_screen_block)?)));
    }
    // Runs after the block's handler, whose value no longer names the column.
    handlers.push(lol_html::element!("img[sizes], source[sizes]", |el| {
        Ok(restate(el, &|sizes| sizes_in_page_column(sizes, content_width))?)
    }));
    let mut out = Vec::with_capacity(body.len() + 64);
    let mut rewriter = lol_html::HtmlRewriter::new(
        lol_html::Settings { element_content_handlers: handlers, ..lol_html::Settings::default() },
        |c: &[u8]| out.extend_from_slice(c),
    );
    let wrote = rewriter.write(OPEN.as_bytes()).and_then(|()| rewriter.write(body.as_bytes())).and_then(|()| rewriter.write(CLOSE.as_bytes()));
    if wrote.is_err() || rewriter.end().is_err() {
        return body.to_string();
    }
    String::from_utf8(out)
        .ok()
        .and_then(|s| s.strip_prefix(OPEN).and_then(|s| s.strip_suffix(CLOSE)).map(str::to_string))
        .unwrap_or_else(|| body.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_and_band_without_a_width_get_screen() {
        let body = r#"<h2>Top</h2><div class="moss-grid" data-columns="2">x</div><div class="band"><p>y</p></div>"#;
        let out = widened(body, SCREEN);
        assert!(out.contains(r#"<div class="moss-grid" data-columns="2" data-width="screen">"#), "{out}");
        assert!(out.contains(r#"<div class="band" data-width="screen">"#), "{out}");
        assert!(out.starts_with("<h2>Top</h2>"), "{out}");
    }

    #[test]
    fn a_block_with_its_own_width_is_left_alone() {
        let body = r#"<div class="moss-grid" data-width="body">x</div><div class="band" data-width="wide">y</div>"#;
        assert_eq!(widened(body, SCREEN), body);
    }

    #[test]
    fn nested_and_component_blocks_are_left_alone() {
        let body = r#"<div class="band"><div class="moss-grid">in</div></div><div class="moss-gallery">g</div><div>plain</div>"#;
        let out = widened(body, SCREEN);
        assert!(out.contains(r#"<div class="moss-grid">in</div>"#), "{out}");
        assert!(out.contains(r#"<div class="moss-gallery">g</div>"#), "{out}");
        assert!(out.contains("<div>plain</div>"), "{out}");
    }

    /// One `sizes` per picture, in document order.
    fn sizes_of(html: &str) -> Vec<&str> {
        html.split(r#"sizes=""#).skip(1).filter_map(|rest| rest.split('"').next()).collect()
    }

    /// A paragraph's picture, a two-column grid holding a picture and a card's
    /// cover, a band's picture, and a grid the author kept at the column.
    fn page() -> String {
        use moss_core::contract::sizes::{sizes_for_grid_cell, SIZES_BODY};
        let cell = sizes_for_grid_cell(2, None, None, None);
        format!(
            r#"<p><img sizes="{SIZES_BODY}"></p><div class="moss-grid" data-columns="2"><div><img sizes="{cell}"></div><a class="moss-card"><picture><source sizes="{cell}"><img></picture></a></div><div class="band"><picture><source sizes="{SIZES_BODY}"><img></picture></div><div class="moss-grid" data-width="body"><img sizes="{cell}"></div>"#
        )
    }

    #[test]
    fn a_screen_page_restates_a_widened_block_for_the_window_and_the_rest_for_the_column() {
        let out = widened(&page(), SCREEN);
        let half_window = "auto, (min-width: 48rem) calc(100vw / 2), 100vw";
        assert_eq!(
            sizes_of(&out),
            [
                "auto, (min-width: 48rem) min(1200px, 100vw), 100vw",
                half_window,
                half_window,
                "auto, 100vw",
                "auto, (min-width: 48rem) calc(min(1200px, 100vw) / 2), 100vw",
            ],
            "{out}"
        );
    }

    #[test]
    fn a_wide_page_restates_every_picture_for_its_column() {
        let out = apply(page(), Some("wide"));
        let half = "auto, (min-width: 48rem) calc(min(56.25rem, 100vw) / 2), 100vw";
        let column = "auto, (min-width: 48rem) min(56.25rem, 100vw), 100vw";
        assert_eq!(sizes_of(&out), [column, half, half, column, half], "{out}");
        assert!(!out.contains("data-width=\"screen\""), "{out}");
    }

    #[test]
    fn a_page_at_the_default_column_is_untouched() {
        assert_eq!(apply(page(), None), page());
        assert_eq!(apply(page(), Some("unknown")), page());
    }

    #[test]
    fn screen_page_takes_the_full_column() {
        assert_eq!(column_preset("screen"), "full");
        assert_eq!(column_preset("wide"), "wide");
    }
}
