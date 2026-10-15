use super::site_limit_message;

#[test]
fn names_the_limit_and_the_servers_contact() {
    let msg = site_limit_message(3, Some("help@example.com"));
    assert!(msg.contains('3') && msg.contains("help@example.com"), "{msg}");
}

#[test]
fn without_a_contact_the_message_is_one_sentence() {
    let with = site_limit_message(3, Some("help@example.com"));
    let without = site_limit_message(3, None);
    assert!(with.starts_with(&without) && with.len() > without.len());
    assert!(!without.contains('@'), "{without}");
}

#[test]
fn exact_text_in_english() {
    assert_eq!(site_limit_message(1, None), "Your hosting plan includes 1 site.");
    assert_eq!(
        site_limit_message(3, Some("help@example.com")),
        "Your hosting plan includes 3 sites. To add more, write to help@example.com."
    );
}
