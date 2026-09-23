use super::*;

#[test]
fn place_breadcrumb_walks_the_full_parent_chain_oldest_first() {
    let pairs = vec![
        ("Japan".to_string(), "/places/japan/".to_string()),
        ("Kansai".to_string(), "/places/kansai/".to_string()),
    ];
    let html = render_breadcrumb(&pairs).expect("non-empty chain renders");
    assert!(html.starts_with(r#"<nav class="moss-place-breadcrumb">"#));
    let japan_pos = html.find("Japan").unwrap();
    let kansai_pos = html.find("Kansai").unwrap();
    assert!(japan_pos < kansai_pos, "oldest ancestor renders first: {html}");
    assert!(html.contains(r#"<a href="/places/japan/" class="breadcrumb-segment">Japan</a>"#));
    assert!(html.contains(r#"<span class="breadcrumb-separator">/</span>"#));
}

#[test]
fn place_breadcrumb_is_none_for_a_root_place() {
    assert_eq!(render_breadcrumb(&[]), None);
}

#[test]
fn place_children_lists_direct_children_with_roll_up_inclusive_counts() {
    let children = vec![
        ("Kyoto".to_string(), "/places/kyoto/".to_string(), 3),
        ("Osaka".to_string(), "/places/osaka/".to_string(), 1),
    ];
    let html = render_children(&children).expect("non-empty children render");
    assert!(html.starts_with(r#"<ul class="moss-place-children">"#));
    assert!(html.contains(r#"<li><a href="/places/kyoto/">Kyoto (3)</a></li>"#));
    assert!(html.contains(r#"<li><a href="/places/osaka/">Osaka (1)</a></li>"#));
}

#[test]
fn place_children_is_none_for_a_leaf_place() {
    assert_eq!(render_children(&[]), None);
}
