//! Follow HTTP redirects by hand, so the caller can veto every hop.
//!
//! A validated public host can answer 3xx with a target on the private network;
//! an agent that follows redirects itself connects there before the caller sees
//! it. Build the agent with `redirects(0)` ([`super::proxy::proxied_ureq_agent_no_redirects`])
//! and let [`get_following_redirects`] walk the chain.

/// Redirect hops followed before giving up: ureq's own default, so a legitimate
/// chain (http to https, then a trailing-slash or www canonicalization) still resolves.
pub(crate) const MAX_REDIRECT_HOPS: u32 = 5;

/// `agent.get(start_url)`, resolving each `Location` against the URL that sent
/// it, passing it through `rewrite`, then to `validate` before it is requested.
/// A refused hop or an over-long chain is `Err(Status(400, ..))` carrying the
/// reason in its status text; a 3xx with no usable `Location` is returned as-is.
///
/// `accept` and `referer` are sent on every hop.
pub(crate) fn get_following_redirects(
    agent: &ureq::Agent,
    start_url: &str,
    user_agent: &str,
    accept: Option<&str>,
    referer: Option<&str>,
    validate: impl Fn(&str) -> Result<(), String>,
    rewrite: impl Fn(String) -> String,
) -> Result<ureq::Response, ureq::Error> {
    // 400 is a permanent refusal that the retry layer never repeats
    // (`should_retry_status` retries only 429/5xx), and its error text is
    // `"{code} {status_text}"`, so the reason travels in the status text.
    const REFUSED_STATUS: u16 = 400;
    // A status line must be printable ASCII; a refusal reason is free text.
    let refused = |status_text: &str| {
        let status_text: String = status_text
            .chars()
            .map(|c| if c.is_ascii() && !c.is_ascii_control() { c } else { '-' })
            .collect();
        ureq::Error::Status(
            REFUSED_STATUS,
            ureq::Response::new(REFUSED_STATUS, &status_text, "")
                .expect("a printable ASCII status line always builds"),
        )
    };

    let mut current = start_url.to_string();
    for _ in 0..=MAX_REDIRECT_HOPS {
        let mut req = agent.get(&current).set("User-Agent", user_agent);
        if let Some(accept) = accept {
            req = req.set("Accept", accept);
        }
        if let Some(referer) = referer {
            req = req.set("Referer", referer);
        }
        let response = req.call()?;
        if !(300..400).contains(&response.status()) {
            return Ok(response);
        }
        let Some(location) = response.header("Location") else {
            return Ok(response);
        };
        let next = match url::Url::parse(&current).and_then(|base| base.join(location)) {
            Ok(joined) => rewrite(joined.to_string()),
            Err(_) => return Ok(response),
        };
        if let Err(msg) = validate(&next) {
            return Err(refused(&format!("redirected to a disallowed URL: {msg}")));
        }
        current = next;
    }
    Err(refused(&format!("too many redirects (> {MAX_REDIRECT_HOPS})")))
}
