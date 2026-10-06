use super::*;
use crate::types::assets::AssetState;
use crate::types::content::MediaMetadata;

const DIMS: (u32, u32) = (2400, 1800);

fn vault_with(plate_ext: &str) -> ProjectStructure {
    ProjectStructure {
        root_path: "/vault".to_string(),
        markdown_files: vec![],
        html_files: vec![],
        image_files: vec![MediaMetadata {
            path: format!("plate.{plate_ext}"),
            file_type: plate_ext.to_string(),
            dimensions: Some(DIMS),
            ..Default::default()
        }],
        video_files: vec![],
        notebook_files: vec![],
        other_files: vec![],
        total_files: 0,
        homepage_file: None,
        ffmpeg_bin_path: None,
        evicted_count: 0,
        evicted_paths: Vec::new(),
        has_content_folders: false,
        has_language_trees: false,
        passthrough_roots: std::collections::HashSet::new(),
        dirs: Vec::new(),
    }
}

fn item(skip: Option<SkipReason>) -> ImageConversionItem {
    ImageConversionItem {
        source_path: PathBuf::from("plate.jpg"),
        source_oid: String::new(),
        ext: "jpg".to_string(),
        dimensions: Some(DIMS),
        skip,
        fingerprint: None,
    }
}

fn rung_urls() -> Vec<String> {
    moss_core::asset_paths::ladder_rungs(DIMS.0, DIMS.1, false)
        .iter()
        .map(|w| moss_core::asset_paths::to_webp_rung("plate.jpg", *w))
        .collect()
}

fn promise(registry: &AssetRegistry, items: Vec<ImageConversionItem>, collisions: HashMap<String, PathBuf>) -> Vec<ImageConversionItem> {
    promise_image_variants(Some(registry), items, &vault_with("jpg"), &HashMap::new(), &collisions, "/vault")
}

#[test]
fn a_source_that_will_never_encode_settles_every_promised_url_failed() {
    // The synthesizer has already emitted `<source srcset="plate.webp …">`
    // and the rung candidates. Settling them Failed is what lets the
    // post-seal degrade pass drop the `<source>` — before this the
    // URLs stayed unregistered and 404ed on the published site.
    let registry = AssetRegistry::new();
    let premise = rung_urls();
    assert!(!premise.is_empty(), "premise: {DIMS:?} carries a ladder");

    let encode = promise(&registry, vec![item(Some(SkipReason::Cmyk))], HashMap::new());

    assert!(encode.is_empty(), "nothing for the encoder");
    for url in std::iter::once("plate.webp".to_string()).chain(premise) {
        match registry.get(&url) {
            Some(AssetState::Failed(why)) => assert!(why.contains("CMYK"), "{url}: {why}"),
            other => panic!("{url} must be Failed, got {other:?}"),
        }
        assert!(registry.source_passthrough(&url).is_none(), "{url}: a Failed variant is never passed through");
    }
}

#[test]
fn a_source_still_in_the_cloud_is_promised_with_a_passthrough() {
    // The bytes are on their way down, so the promise is kept
    // Pending and the preview serves the original at every variant URL.
    let registry = AssetRegistry::new();

    let encode = promise(&registry, vec![item(Some(SkipReason::SourceInTheCloud))], HashMap::new());

    assert!(encode.is_empty(), "the encoder would take an EDEADLK on it");
    for url in std::iter::once("plate.webp".to_string()).chain(rung_urls()) {
        assert!(matches!(registry.get(&url), Some(AssetState::Pending(_))), "{url} must be Pending");
        assert_eq!(registry.source_passthrough(&url), Some(PathBuf::from("/vault/plate.jpg")));
    }
}

#[test]
fn a_user_file_on_a_rung_url_is_never_marked() {
    // `rung_collision_map`: a vault file literally named like a rung wins.
    // The never-encoded branch must honour the same rule the promise does,
    // or the user's own file would be served as a warning tile.
    let registry = AssetRegistry::new();
    let theirs = rung_urls().remove(0);
    let collisions = HashMap::from([(theirs.clone(), PathBuf::from("/vault/theirs.webp"))]);

    promise(&registry, vec![item(Some(SkipReason::Cmyk))], collisions);

    assert!(registry.get(&theirs).is_none(), "{theirs} belongs to the user");
    assert!(matches!(registry.get("plate.webp"), Some(AssetState::Failed(_))));
}

#[test]
fn an_encodable_item_is_promised_and_handed_to_the_encoder() {
    let registry = AssetRegistry::new();

    let encode = promise(&registry, vec![item(None)], HashMap::new());

    assert_eq!(encode.len(), 1);
    assert!(matches!(registry.get("plate.webp"), Some(AssetState::Pending(_))));
    assert_eq!(registry.source_passthrough("plate.webp"), Some(PathBuf::from("/vault/plate.jpg")));
}

#[test]
fn without_a_registry_the_verdict_still_keeps_the_item_from_the_encoder() {
    // Headless builds have no registry to promise to; the encoder queue must
    // still exclude what this build cannot encode.
    let encode = promise_image_variants(
        None,
        vec![item(Some(SkipReason::Cmyk)), item(None)],
        &vault_with("jpg"),
        &HashMap::new(),
        &HashMap::new(),
        "/vault",
    );
    assert_eq!(encode.len(), 1);
    assert!(encode[0].skip.is_none());
}
