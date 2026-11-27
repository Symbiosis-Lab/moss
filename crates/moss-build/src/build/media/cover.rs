//! A card's pictures as the build draws them: through the site's media scan
//! (sizes, variants, LQIP) and with their colour band read from the image.

use std::path::Path;

use crate::build::media::dimensions::MediaDimensionLookup;

pub use moss_core::page::card::{detect_cover_type, CoverType};
pub use moss_core::render::cards::cover_html;
pub(crate) use moss_core::media::html_escape;

/// The build's [`CardPictures`](moss_core::render::cards::CardPictures).
/// `root` is the project root a cover's colour is read under; `None` disables
/// file reads, so an image cover then colours only through its `|color=`.
pub struct BuildPictures<'a> {
    pub root: Option<&'a Path>,
    pub lookup: Option<&'a MediaDimensionLookup>,
}

impl moss_core::render::cards::CardPictures for BuildPictures<'_> {
    fn assets(&self) -> Option<&moss_core::asset_snapshot::AssetSnapshot> {
        self.lookup.map(MediaDimensionLookup::asset_snapshot)
    }
    fn card_color(&self, cover: Option<&str>, cover_type: Option<CoverType>) -> Option<String> {
        crate::build::components::color_extract::resolve_card_color(cover, cover_type, self.root, self.lookup)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use moss_core::render::image::Loading;


    // ── BUG 6: cover under a dir-override resolves real dims ─────

    #[test]
    fn render_image_cover_under_dir_override_emits_real_dims_not_fallback() {
        // The reviewer's scenario: a folder mapped to a custom slug via
        // dir_overrides. The emitter produces the cover URL under the OVERRIDE
        // slug (`assets/euro/...`), while the scan inventory keys dims under the
        // raw source path (`assets/Europe - A Prophecy/...`). The lookup carries
        // the overrides so `build_asset_snapshot` additively indexes the dims
        // under the override-slug output-URL key the cover URL probes with.
        // Without threading the overrides (an empty map), the snapshot would
        // only hold source-key + base-slug (`europe-a-prophecy`), the
        // override-slug probe would MISS, and the 800x600 fallback would fire.
        use crate::build::media::dimensions::MediaDimensionLookup;
        use crate::types::content::MediaMetadata;
        let meta = MediaMetadata {
            is_animated: false,
            path: "assets/Europe - A Prophecy/e-006.jpg".to_string(),
            file_type: "jpg".to_string(),
            size: 0,
            modified: None,
            dimensions: Some((4515, 6158)),
            dominant_color: None,
            lqip_data_uri: None,
        };
        let mut overrides = std::collections::HashMap::new();
        overrides.insert("assets/Europe - A Prophecy".to_string(), "euro".to_string());
        let lookup = MediaDimensionLookup::new(&[meta], &[], &overrides, None, &Default::default());

        // The emitter resolves the cover to the OVERRIDE slug.
        let html = cover_html(
                "assets/euro/e-006.jpg",
                CoverType::Image,
                "alt",
                "cover",
                &Default::default(),
                false,
                Some(&lookup).map(|l| l.asset_snapshot()),
                Loading::Lazy,
                None,
            );
        assert!(
            html.contains(r#"width="4515""#) && html.contains(r#"height="6158""#),
            "override-slug cover must carry real dims, got: {}", html
        );
        assert!(
            !(html.contains(r#"width="800""#) && html.contains(r#"height="600""#)),
            "800x600 fallback must NOT fire for a dir-override cover, got: {}", html
        );
    }
}
