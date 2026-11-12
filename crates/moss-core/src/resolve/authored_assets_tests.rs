use super::*;
use crate::content_graph::ContentGraph;
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
    let assets = ContentGraph::from_paths(assets);
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
fn an_unknown_block_around_a_gallery_leaves_the_gallery_to_the_parser() {
    // Same colon count outside and in: the gallery's closer is the first bare
    // colon line, the unknown block's is the second, and the renderer's
    // parser and the rename scanner agree on who owns the image.
    let source = ":::typo\n:::gallery\nowned.png\n:::\n:::\n";
    assert_eq!(texts(source), ["owned.png"]);
    // Hand-written pairing: one gallery block, closed by the first `:::`, so
    // exactly one span. A scanner that mis-paired would see none or two.
    let spans = crate::ast::shortcode_extract::shortcode_asset_spans(source);
    assert_eq!(spans.len(), 1, "{spans:?}");
    assert_eq!(spans[0].path, "owned.png");
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

    // A bare colon line closes the innermost open block whatever its length:
    // the first closer is the gallery's, and the grid it sits in is left
    // without one, so only the gallery is a block.
    let one_closer = ":::grid\n:::gallery\nnot-rendered.png\n:::\n";
    assert_eq!(
        crate::ast::shortcode_extract::extract_shortcodes(one_closer).extracted.len(),
        1,
        "the only closer belongs to the gallery"
    );
    // With both closers present the grid is a block around the gallery.
    let both = ":::grid\n:::gallery\nrendered.png\n:::\n:::\n";
    assert_eq!(texts(both), ["rendered.png"]);
}

#[test]
fn structural_boundaries_match_multiline_attrs_hero_priority_and_inert_openers() {
    let source = ":::hero {\n  image=attr.png\n}\nbody.png\n:::\n\n:::hero positional.jpg\nbody-ignored.png\n:::\n\n<!--\n:::hero\ninert.png\n:::\n-->\n";
    assert_eq!(crate::ast::shortcode_extract::extract_shortcodes(source).extracted.len(), 2);
    assert_eq!(texts(source), ["attr.png", "positional.jpg"]);

    let malformed = ":::hero {\n  image=never.png\n";
    assert!(refs(malformed).is_empty());
}

/// Each line here was built and read back: seven reach the page as literal
/// text, the four expected below as a picture.
#[test]
fn markdown_in_a_raw_html_block_is_no_candidate() {
    let source = "\
<div>
<pre><span>![in a wrapped pre](wrapped-pre.jpg)</span></pre>
![[wiki-in-div.jpg]]
</div>

<pre>
![top pre](top-pre.jpg)

![past a blank line in a pre](pre-blank.jpg)
</pre>

<script>
const s = \"![in script](in-script.jpg)\";
</script>

<style>
/* ![in style](in-style.jpg) */
</style>

Inline <code>![inline code](inline-code.jpg)</code> is Markdown.

<span>hi</span> ![after a tag](after-tag.jpg)

<div>
<pre>
![wrapped](wrapped.jpg)

![past the wrapper's blank line](wrapper-blank.jpg)
</pre>
</div>

<div>
:::grid
![in a shortcode](in-shortcode.jpg)
:::
</div>
";
    assert_eq!(
        texts(source),
        ["inline-code.jpg", "after-tag.jpg", "wrapper-blank.jpg", "in-shortcode.jpg"]
    );
}

#[test]
fn a_raw_media_tag_names_its_src_as_a_file() {
    let source = "<img src=\"img/gone.svg\" alt=\"\">\n\nText <IMG alt='' SRC='img/here.svg'> and <video src=clip.mp4 controls></video>.\n\n<img alt=\"no source\">\n\n`<img src=\"shown.png\">`\n\n<!-- <img src=\"commented.png\"> -->\n\n<img src=\"https://example.org/far.png\">\n";
    assert_eq!(texts(source), ["img/gone.svg", "img/here.svg", "clip.mp4", "https://example.org/far.png"]);
    assert_eq!(missing(source, &["img/here.svg"]), ["img/gone.svg", "clip.mp4"]);
    let gone = &refs(source)[0].raw;
    assert_eq!(&source[gone.ref_from..gone.ref_to], "img/gone.svg");
    assert_eq!(gone.byte_from, 0);
}

#[test]
fn media_tags_shown_as_code_are_not_raw_media() {
    let md = "A live one: <img src=\"live.jpg\">\n\n\
              ```html\n<img src=\"fenced.jpg\">\n```\n\n\
              ~~~\n<video src=\"tilde.mp4\">\n~~~\n\n\
              Inline: `<audio src=\"inline.mp3\">`.\n\n\
              <!-- <img src=\"commented.jpg\"> -->\n\n\
              \x20   <img src=\"indented.jpg\">\n";
    let hits = raw_media_tags(md);
    assert_eq!(hits.len(), 1, "only the live tag is raw media: {hits:?}");
    assert_eq!(hits[0].byte_offset, "A live one: ".len());
}

#[test]
fn media_tag_in_a_raw_html_block_is_still_raw_media() {
    let hits = raw_media_tags("<div>\n<img src=\"block.jpg\">\n</div>\n");
    assert_eq!(hits.len(), 1, "an HTML block is live markup, not code: {hits:?}");
}

#[test]
fn detects_raw_img_tag() {
    let md = "some text <img src=\"x.jpg\"> more text";
    let hits = raw_media_tags(md);
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].tag, "img");
    assert_eq!(hits[0].byte_offset, "some text ".len());
}

#[test]
fn detects_video_tag() {
    let md = "<video src=\"x.mp4\">";
    let hits = raw_media_tags(md);
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].tag, "video");
    assert_eq!(hits[0].byte_offset, 0);
}

/// `![[track.mp3]]` synthesizes an audio player, so a hand-typed `<audio>`
/// loses that as a raw `<img>` loses its placeholder.
#[test]
fn detects_audio_tag() {
    let md = "<audio src=\"x.mp3\" controls></audio>";
    let hits = raw_media_tags(md);
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].tag, "audio");
    assert_eq!(hits[0].byte_offset, 0);
}

#[test]
fn no_detection_for_markdown_image() {
    let md = "![alt](photo.jpg) plain markdown\n![[file.png]] embed";
    assert!(raw_media_tags(md).is_empty());
}

#[test]
fn detects_uppercase_tag() {
    let md = "<IMG src=\"x.jpg\">";
    let hits = raw_media_tags(md);
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].tag, "img");
}

#[test]
fn detects_self_closing_img() {
    let md = "<img/>";
    let hits = raw_media_tags(md);
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].tag, "img");
}

#[test]
fn detects_multiple_tags_in_one_page() {
    let md = "<img src=\"a.jpg\"> middle <video src=\"b.mp4\"></video> <img>";
    let hits = raw_media_tags(md);
    assert_eq!(hits.len(), 3);
    assert_eq!(hits[0].tag, "img");
    assert_eq!(hits[1].tag, "video");
    assert_eq!(hits[2].tag, "img");
}

#[test]
fn does_not_match_similar_identifiers() {
    // `<imgur>`, `<videos>` and `<audiobook>` are not raw media tags -
    // the boundary class (`\s`, `>`, `/`) prevents the false match.
    let md = "<imgur>foo</imgur> <videos>bar</videos> <audiobook>baz</audiobook>";
    assert!(raw_media_tags(md).is_empty());
}

#[test]
fn detects_tag_with_no_attributes() {
    let md = "<img>";
    let hits = raw_media_tags(md);
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].tag, "img");
}

/// A page's own script, style or form text can spell a media tag
/// (`'<img src="' + url + '">'`); the browser reads it as text.
#[test]
fn a_media_tag_inside_script_style_or_textarea_is_text() {
    let md = "<script>\nel.innerHTML = '<img src=\"' + url + '\">';\nconst t = `<video src=\"${clip}\">`;\n</script>\n\n\
              <style>/* <img src=\"in-style.png\"> */</style>\n\n\
              <textarea><audio src=\"in-form.mp3\"></textarea>\n\n\
              <img src=\"live.png\">\n\n<script>\n'<img src=\"never-closed.png\">'\n";
    let hits = raw_media_tags(md);
    assert_eq!(hits.len(), 1, "{hits:?}");
    assert_eq!(texts(md), ["live.png"]);
}

/// Only a file the folder should hold can be missing.
#[test]
fn a_raw_src_that_names_no_file_or_a_present_one_is_not_missing() {
    let source = "<img src=\"\">\n\n<img src=\"data:image/gif;base64,R0lGODlhAQABAAAAACw=\">\n\n<img src=\"#\">\n\n\
                  <img src=\"//cdn.example.org/far.png\">\n\n<img src=\"/img/here.svg\">\n\n<img src=\"img/here.svg?v=2\">\n";
    assert_eq!(missing(source, &["img/here.svg"]), Vec::<String>::new());
}
