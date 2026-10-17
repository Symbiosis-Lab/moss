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
    assert_eq!(site_limit_message(1, None), "Your mosspub.com hosting includes 1 site.");
    assert_eq!(
        site_limit_message(3, Some("help@example.com")),
        "Your mosspub.com hosting includes 3 sites. To add more, write to help@example.com."
    );
}

#[test]
fn a_limit_of_zero_says_hosting_is_closed() {
    let closed = "mosspub.com hosting isn't taking new sites right now.";
    assert_eq!(site_limit_message(0, None), closed);
    assert_eq!(
        site_limit_message(0, Some("help@example.com")),
        format!("{closed} Write to help@example.com to hear when it reopens.")
    );
}
