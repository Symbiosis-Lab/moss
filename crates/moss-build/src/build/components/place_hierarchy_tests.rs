use super::*;

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
