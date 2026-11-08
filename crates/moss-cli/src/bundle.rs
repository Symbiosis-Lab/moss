//! The browser UI bundle `build.rs` embeds.
use moss_build::ops::serve::ui::UiAsset;

include!(concat!(env!("OUT_DIR"), "/ui_bundle.rs"));

pub fn require_bundle() -> Result<&'static [UiAsset], String> {
    match MISSING {
        Some(marker) => Err(format!("{marker}: this moss-cli was built without the browser editor; build it with `bash scripts/build-moss-cli.sh`, which builds the editor's bundle before cargo")),
        None => Ok(ASSETS),
    }
}

#[cfg(all(test, not(moss_ui_bundle_missing)))]
mod tests {
    use super::*;
    use std::io::Read;

    #[test]
    fn embedded_bundle_is_gzipped_with_entry_and_hashed_script() {
        let assets = require_bundle().unwrap();
        let entry = assets.iter().find(|a| a.path == "index.html").expect("entry page");
        assert_eq!(entry.content_type, "text/html; charset=utf-8");
        assert!(!entry.immutable);
        assert!(assets.iter().any(|a| a.path.ends_with(".js")
            && a.immutable
            && a.content_type == "text/javascript; charset=utf-8"));
        for asset in assets {
            assert!(asset.gzip, "{}", asset.path);
            let mut decoded = Vec::new();
            flate2::read::GzDecoder::new(asset.bytes).read_to_end(&mut decoded).unwrap();
            assert!(!decoded.is_empty(), "{}", asset.path);
        }
    }
}
