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
//! A picture inside a widened block was given a `sizes` for the content
//! column, so the same pass restates it for the block it is now drawn in.

/// The `content_width` value that sets this default; the page's column is then
/// the `full` one.
pub(crate) const SCREEN: &str = "screen";

/// What `<body data-content-width>` says for a page's `content_width`: the
/// default only changes the blocks, so the column is the `full` column.
pub(crate) fn column_preset(content_width: &str) -> &str {
    if content_width == SCREEN { "full" } else { content_width }
}

/// `body`, with the `screen` default applied when the page's resolved
/// `content_width` asks for it.
pub(crate) fn apply(body: String, content_width: Option<&str>) -> String {
    if content_width == Some(SCREEN) { screen_by_default(&body) } else { body }
}

/// `body` with `data-width="screen"` added to every top-level grid and
/// fenced-div band that has no width of its own, and the `sizes` of the
/// pictures inside restated for that width. The selectors mirror the
/// stylesheet's `article.container > …` escape rules, so only a block the
/// escape reaches is touched.
pub(crate) fn screen_by_default(body: &str) -> String {
    const OPEN: &str = r#"<article class="container">"#;
    const CLOSE: &str = "</article>";
    // The blocks the default widens. Selectors match the source's attributes,
    // so the pictures inside are still found once the block carries its width.
    const BLOCKS: [&str; 2] = [
        "article.container > .moss-grid:not([data-width])",
        r#"article.container > div[class]:not([data-width]):not([class^="moss-"]):not([class*=" moss-"])"#,
    ];
    let pictures = BLOCKS.map(|block| format!("{block} img[sizes], {block} source[sizes]")).join(", ");
    let mut out = Vec::with_capacity(body.len() + 64);
    let mut rewriter = lol_html::HtmlRewriter::new(
        lol_html::Settings {
            element_content_handlers: vec![
                lol_html::element!(BLOCKS.join(", "), |el| Ok(el.set_attribute("data-width", "screen")?)),
                lol_html::element!(pictures, |el| {
                    let restated = el.get_attribute("sizes")
                        .and_then(|sizes| moss_core::contract::sizes::sizes_in_screen_block(&sizes));
                    if let Some(sizes) = restated {
                        el.set_attribute("sizes", &sizes)?;
                    }
                    Ok(())
                }),
            ],
            ..lol_html::Settings::default()
        },
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
        let out = screen_by_default(body);
        assert!(out.contains(r#"<div class="moss-grid" data-columns="2" data-width="screen">"#), "{out}");
        assert!(out.contains(r#"<div class="band" data-width="screen">"#), "{out}");
        assert!(out.starts_with("<h2>Top</h2>"), "{out}");
    }

    #[test]
    fn a_block_with_its_own_width_is_left_alone() {
        let body = r#"<div class="moss-grid" data-width="body">x</div><div class="band" data-width="wide">y</div>"#;
        assert_eq!(screen_by_default(body), body);
    }

    #[test]
    fn nested_and_component_blocks_are_left_alone() {
        let body = r#"<div class="band"><div class="moss-grid">in</div></div><div class="moss-gallery">g</div><div>plain</div>"#;
        let out = screen_by_default(body);
        assert!(out.contains(r#"<div class="moss-grid">in</div>"#), "{out}");
        assert!(out.contains(r#"<div class="moss-gallery">g</div>"#), "{out}");
        assert!(out.contains("<div>plain</div>"), "{out}");
    }

    #[test]
    fn pictures_in_a_widened_block_get_sizes_for_it() {
        use moss_core::contract::sizes::{sizes_for_grid_cell, SIZES_BODY};
        let cell = sizes_for_grid_cell(2, None);
        let body = format!(
            r#"<p><img sizes="{SIZES_BODY}"></p><div class="moss-grid" data-columns="2"><div><img sizes="{cell}"></div></div><div class="band"><picture><source sizes="{SIZES_BODY}"><img></picture></div><div class="moss-grid" data-width="body"><img sizes="{cell}"></div><p><img sizes="{SIZES_BODY}"></p>"#
        );
        let out = screen_by_default(&body);
        let sizes: Vec<&str> = out.split(r#"sizes=""#).skip(1).filter_map(|rest| rest.split('"').next()).collect();
        let screen_cell = sizes_for_grid_cell(2, Some("screen"));
        assert_eq!(sizes, [SIZES_BODY, screen_cell.as_str(), "auto, 100vw", cell.as_str(), SIZES_BODY], "{out}");
    }

    #[test]
    fn screen_page_takes_the_full_column() {
        assert_eq!(column_preset("screen"), "full");
        assert_eq!(column_preset("wide"), "wide");
    }
}
