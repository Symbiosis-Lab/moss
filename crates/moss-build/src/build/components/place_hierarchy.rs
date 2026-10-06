//! A place page's direct descendants with counts, rendered from resolved
//! term data shared by authored and generated place pages.

use crate::build::media::cover::html_escape;

/// `<ul class="moss-place-children">`, one `<li>` per direct child with its
/// roll-up-inclusive member count. `None` for no children (a leaf place, or
/// any non-place term).
pub fn render_children(children: &[(String, String, usize)]) -> Option<String> {
    if children.is_empty() {
        return None;
    }
    let items: String = children
        .iter()
        .map(|(display, url, count)| {
            format!(
                r#"<li><a href="{}">{} ({})</a></li>"#,
                html_escape(url),
                html_escape(display),
                count,
            )
        })
        .collect();
    Some(format!(r#"<ul class="moss-place-children">{}</ul>"#, items))
}

#[cfg(test)]
#[path = "place_hierarchy_tests.rs"]
mod tests;
