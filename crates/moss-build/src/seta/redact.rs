//! What the seta client may write to its log.

/// `url` with the session id of an `/auth/status/<id>` path replaced by `…`.
pub(super) fn session_id(url: &str) -> String {
    match url.split_once("/auth/status/") {
        Some((head, rest)) => {
            let tail = rest.find(['?', '#', '/']).map_or("", |i| &rest[i..]);
            format!("{head}/auth/status/…{tail}")
        }
        None => url.to_string(),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn only_the_session_id_is_replaced() {
        assert_eq!(super::session_id("https://h/auth/status/abc123?x=1"), "https://h/auth/status/…?x=1");
        assert_eq!(super::session_id("https://h/api/sites/abc"), "https://h/api/sites/abc");
    }
}
