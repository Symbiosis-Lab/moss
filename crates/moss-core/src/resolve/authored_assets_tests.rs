use super::*;
use crate::resolve::asset_class::FakeAssetIndex;
use crate::resolve::folder_class::FakeFolderIndex;
use crate::resolve::link_class::FakeUrlIndex;

fn refs(source: &str) -> Vec<AuthoredAssetRef> {
    refs_with_config(source, &crate::ast::parser::ParseConfig::default())
}

fn refs_with_config(
    source: &str,
    parse_config: &crate::ast::parser::ParseConfig,
) -> Vec<AuthoredAssetRef> {
    extract_authored_asset_refs(source, parse_config)
}

fn texts(source: &str) -> Vec<String> {
    refs(source)
        .into_iter()
        .map(|reference| reference.raw.text)
        .collect()
}

fn missing(source: &str, assets: &[&str]) -> Vec<String> {
    let assets = FakeAssetIndex::new(assets);
    let folders = FakeFolderIndex::new();
    let urls = FakeUrlIndex::new();
    let context = ReferenceContext {
        assets: &assets,
        folders: &folders,
        urls: &urls,
    };
    refs(source)
        .into_iter()
        .filter(|reference| reference.missing_asset("page.md", &context).is_some())
        .map(|reference| reference.raw.text)
        .collect()
}

#[test]
fn markdown_and_wikilink_candidates_keep_their_authored_spans() {
    let source = "![alt](missing.png) [file](missing.pdf) ![[clip.mp4|wide]] ![[song.m4a#chorus|caption]]";
    let refs = refs(source);
    assert_eq!(texts(source), ["missing.png", "missing.pdf", "clip.mp4", "song.m4a#chorus"]);
    assert!(refs[0].is_embed);
    assert!(!refs[1].is_embed);
    assert!(refs[2].is_embed);
    assert!(refs[3].is_embed);
    for reference in refs {
        assert_eq!(
            &source[reference.raw.ref_from..reference.raw.ref_to],
            reference.raw.text
        );
    }
}

#[test]
fn parser_aligned_shortcodes_cover_attr_positional_body_and_nested_forms() {
    let source = ":::hero hero.jpg|cover\nbody\n:::\n\n:::hero { image=attr.png }\nbody.jpg\n:::\n\n:::hero\nbody.png\n![alt](body-2.jpg)\ncaption\n:::\n\n:::{.wrap}\n::::gallery\ngallery.jpg\n![[gallery-2.png|cover]]\n![alt](gallery-3.webp)\n::::\n:::\n";
    let refs = refs(source);
    assert_eq!(
        refs.iter().map(|reference| reference.raw.text.as_str()).collect::<Vec<_>>(),
        [
            "hero.jpg",
            "attr.png",
            "body.png",
            "body-2.jpg",
            "gallery.jpg",
            "gallery-2.png",
            "gallery-3.webp",
        ]
    );
    for reference in refs {
        assert_eq!(
            &source[reference.raw.ref_from..reference.raw.ref_to],
            reference.raw.text
        );
        assert!(reference.is_embed);
    }
}

#[test]
fn repeated_refs_are_distinct_but_markdown_and_gallery_overlap_once() {
    let source = ":::gallery\n![a](same.png)\nsame.png\n:::\n\n![b](same.png)\n";
    let refs = refs(source);
    let same: Vec<_> = refs
        .iter()
        .filter(|reference| reference.raw.text == "same.png")
        .collect();
    assert_eq!(same.len(), 3, "one occurrence per physical destination");
    assert_ne!(same[0].raw.ref_from, same[1].raw.ref_from);
    assert_ne!(same[1].raw.ref_from, same[2].raw.ref_from);
}

#[test]
fn offsets_survive_utf8_and_crlf() {
    let source = "前言\r\n:::gallery\r\n關於/遺失.png\r\n:::\r\n";
    let reference = refs(source).pop().expect("gallery reference");
    assert_eq!(reference.raw.text, "關於/遺失.png");
    assert_eq!(&source[reference.raw.ref_from..reference.raw.ref_to], "關於/遺失.png");
}

#[test]
fn escapes_and_inert_regions_do_not_create_candidates() {
    let source = "\\[escaped](escaped.png) `![](code.png)`\n<!-- ![](comment.png) -->\n```\n![](fenced.png)\n```\n![](live.png)\n";
    assert_eq!(texts(source), ["live.png"]);
}

#[test]
fn parser_adapter_does_not_adopt_a_rename_scanners_unknown_block_superset() {
    let source = ":::typo\n:::gallery\nfalse-positive.png\n:::\n";
    assert!(
        crate::ast::shortcode_extract::shortcode_asset_spans(source)
            .iter()
            .any(|span| span.path == "false-positive.png"),
        "rename tracking deliberately accepts this over-approximation"
    );
    assert!(refs(source).is_empty());
}

#[test]
fn blocking_verdict_reuses_the_canonical_resolver() {
    let source = "![img](missing.png) [file](missing.pdf) ![[missing.mp4#clip|wide]] [note](missing.md) [unknown](missing.xyz) [external](https://example.com/x) [anchor](#part)";
    assert_eq!(missing(source, &[]), ["missing.png", "missing.mp4#clip"]);
    assert!(missing("![img](present.png)", &["present.png"]).is_empty());
}

#[test]
fn image_reference_definitions_keep_the_definition_destination_span() {
    let present = "![alt][image]\n\n[image]: present.png\n";
    let absent = "![alt][image]\n\n[image]: missing.png\n";
    assert_eq!(texts(present), ["present.png"]);
    assert_eq!(texts(absent), ["missing.png"]);
    assert!(missing(present, &["present.png"]).is_empty());
    assert_eq!(missing(absent, &[]), ["missing.png"]);
    let reference = refs(absent).pop().unwrap();
    assert_eq!(&absent[reference.raw.ref_from..reference.raw.ref_to], "missing.png");
    assert!(reference.is_embed);
}

#[test]
fn only_image_uses_associate_a_reference_definition() {
    let ordinary = "[text][asset]\n\n[asset]: missing.png\n";
    assert!(refs(ordinary).is_empty());
    assert!(missing(ordinary, &[]).is_empty());

    let shared = "![one][asset] and ![two][asset]\n\n[asset]: missing.png\n";
    assert_eq!(texts(shared), ["missing.png"], "one physical definition destination");
    assert_eq!(missing(shared, &[]), ["missing.png"]);

    let mixed = "[text][asset] and ![image][asset]\n\n[asset]: missing.png\n";
    assert_eq!(texts(mixed), ["missing.png"]);
}

#[test]
fn image_definition_association_uses_pulldowns_label_normalization_and_byte_span() {
    let source = "圖 ![alt][IMAGE]\n\n[image]: 關於/遺失.png\n";
    let reference = refs(source).pop().unwrap();
    assert_eq!(reference.raw.text, "關於/遺失.png");
    assert_eq!(&source[reference.raw.ref_from..reference.raw.ref_to], "關於/遺失.png");
}

#[test]
fn collapsed_and_shortcut_image_uses_share_one_utf8_definition_destination() {
    let source = "![ASSET][] and ![asset]\n\n[asset]: 關於/遺失.png\n";
    assert_eq!(texts(source), ["關於/遺失.png"]);
    assert_eq!(missing(source, &[]), ["關於/遺失.png"]);
    let reference = refs(source).pop().unwrap();
    assert_eq!(&source[reference.raw.ref_from..reference.raw.ref_to], "關於/遺失.png");
}

#[test]
fn image_definition_association_uses_the_renderers_math_mode() {
    let source = "$![alt][asset]$\n\n[asset]: missing.png\n";
    let math_off = crate::ast::parser::ParseConfig::default();
    let math_on = crate::ast::parser::ParseConfig { math: true, ..math_off };
    assert_eq!(refs_with_config(source, &math_off).len(), 1);
    assert!(refs_with_config(source, &math_on).is_empty());
}

#[test]
fn structural_modes_share_boundaries_but_keep_their_distinct_ownership_rules() {
    let source = ":::{.css}\n::::gallery\ncss.png\n::::\n:::\n\n:::unknown\n::::gallery\nunknown.png\n::::\n:::\n\n:::grid\n::::gallery\ngrid.png\n::::\n:::\n";
    assert_eq!(
        crate::ast::shortcode_extract::extract_shortcodes(source).extracted.len(),
        3,
        "the renderer's parser owns exactly the three nested galleries"
    );
    assert_eq!(texts(source), ["css.png", "unknown.png", "grid.png"]);

    // The parser gives the first same-arity closer to the outer grid. Its
    // apparent inner gallery is therefore unterminated and owns no media.
    let same_arity = ":::grid\n:::gallery\nnot-rendered.png\n:::\n";
    assert_eq!(
        crate::ast::shortcode_extract::extract_shortcodes(same_arity).extracted.len(),
        1,
        "the same-arity closer belongs to the outer grid"
    );
    assert!(refs(same_arity).is_empty());
    assert!(
        crate::ast::shortcode_extract::shortcode_asset_spans(same_arity)
            .iter()
            .any(|span| span.path == "not-rendered.png"),
        "rename retains its deliberate physical-source superset"
    );
}

#[test]
fn structural_boundaries_match_multiline_attrs_hero_priority_and_inert_openers() {
    let source = ":::hero {\n  image=attr.png\n}\nbody.png\n:::\n\n:::hero positional.jpg\nbody-ignored.png\n:::\n\n<!--\n:::hero\ninert.png\n:::\n-->\n";
    assert_eq!(crate::ast::shortcode_extract::extract_shortcodes(source).extracted.len(), 2);
    assert_eq!(texts(source), ["attr.png", "positional.jpg"]);

    let malformed = ":::hero {\n  image=never.png\n";
    assert!(refs(malformed).is_empty());
}
