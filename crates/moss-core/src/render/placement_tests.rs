use super::*;
use crate::media::AlignSide;

fn placement(width: Option<&'static str>, align: Option<AlignSide>, size: Option<&str>) -> Placement {
    Placement {
        width,
        align,
        size: size.map(str::to_string),
    }
}

#[test]
fn an_empty_placement_contributes_nothing() {
    let a = placement_attrs(&Placement::default());
    assert_eq!(a.data_width_attr, "");
    assert_eq!(a.align_class, None);
    assert_eq!(a.size_style_attr, "");
    assert_eq!(a.class_value("moss-embed"), "moss-embed");
}

#[test]
fn each_field_becomes_its_own_fragment() {
    let a = placement_attrs(&placement(Some("wide"), Some(AlignSide::Right), None));
    assert_eq!(a.data_width_attr, r#" data-width="wide""#);
    assert_eq!(a.align_class, Some("moss-align-right"));
    assert_eq!(a.size_style_attr, "");
    assert_eq!(a.class_value("moss-embed"), "moss-embed moss-align-right");
}

#[test]
fn an_explicit_size_wins_over_a_width_token() {
    // `|wide|40%`: the escape centres a token-sized band with negative
    // margins, so a box the inline style has shrunk to 40% would sit
    // outside the column. The size is the more specific ask; the token goes.
    let a = placement_attrs(&placement(Some("wide"), Some(AlignSide::Right), Some("40%")));
    assert_eq!(a.data_width_attr, "");
    assert_eq!(a.align_class, Some("moss-align-right"));
    assert_eq!(a.size_style_attr, r#" style="width:40%""#);
}

#[test]
fn a_caption_wrapper_carries_the_width_and_the_float() {
    let p = placement(Some("wide"), Some(AlignSide::Left), None);
    let html = wrap_embed_with_caption("<object></object>", &p, "A caption");
    assert_eq!(
        html,
        r#"<figure class="moss-embed-figure moss-align-left" data-width="wide"><object></object><figcaption>A caption</figcaption></figure>"#
    );
}

#[test]
fn a_caption_wrapper_with_only_a_size_still_carries_it() {
    // No width token, no align — just a percent. The figure still has to
    // carry it: it is the outermost element, and a bare <video>/<object>
    // has no width of its own to fall back on.
    let p = placement(None, None, Some("25%"));
    let html = wrap_embed_with_caption("<video></video>", &p, "Cap");
    assert_eq!(
        html,
        r#"<figure class="moss-embed-figure" style="width:25%"><video></video><figcaption>Cap</figcaption></figure>"#
    );
}

#[test]
fn a_caption_is_escaped() {
    let html = wrap_embed_with_caption("<x/>", &Placement::default(), r#"A & <b> "quote""#);
    assert!(html.contains("A &amp; &lt;b&gt; &quot;quote&quot;"), "got: {html}");
}
