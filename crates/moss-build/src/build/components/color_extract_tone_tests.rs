use super::*;

// The pale/dark split is asserted end-to-end by typed_renderers'
// render_hero_tags_a_pale_image_for_a_stronger_scrim and its dark
// counterpart, which drive this function through the renderer with the
// same two colours. The parse-failure path is reachable from there too,
// but indistinguishable — a dark image and an unparseable colour both emit
// no attribute — so it is the one case worth asserting directly.

#[test]
fn is_light_cover_ignores_already_darkened_video_colors() {
    // Video scan stores `hsla(...)`, already WCAG-darkened against white.
    assert_eq!(is_light_cover("hsla(200, 45%, 30%, 1)"), None);
}
