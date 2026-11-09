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
///
/// `reports` is asked about each target before it is validated. When it says
/// yes, the target is never requested: the 3xx response is handed back as it
/// is, and [`redirect_target`] reads where it pointed. This is how a
/// caller learns that a link leads off its site or to a `mailto:` address
/// without connecting there.
pub(crate) fn get_following_redirects(
    agent: &ureq::Agent,
    start_url: &str,
    user_agent: &str,
    accept: Option<&str>,
    referer: Option<&str>,
    validate: impl Fn(&str) -> Result<(), String>,
    rewrite: impl Fn(String) -> String,
    reports: impl Fn(&str) -> bool,
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
    let mut visited = vec![current.clone()];
    for _ in 0..=MAX_REDIRECT_HOPS {
        let mut req = agent.get(&current).set("User-Agent", user_agent);
        if let Some(accept) = accept {
            req = req.set("Accept", accept);
        }
        if let Some(referer) = referer {
            req = req.set("Referer", referer);
        }
        let response = req.call()?;
        let Some(target) = redirect_target(&response) else {
            return Ok(response);
        };
        let next = rewrite(target);
        if reports(&next) {
            return Ok(response);
        }
        if let Err(msg) = validate(&next) {
            return Err(refused(&format!("redirected to a disallowed URL: {msg}")));
        }
        if visited.contains(&next) {
            return Err(refused("the site redirects this address to itself in a loop"));
        }
        visited.push(next.clone());
        current = next;
    }
    Err(refused(&format!("too many redirects (> {MAX_REDIRECT_HOPS})")))
}

/// Where a 3xx response points, resolved against the URL that sent it.
/// `None` for any other response, or one with no usable `Location`.
pub(crate) fn redirect_target(response: &ureq::Response) -> Option<String> {
    if !(300..400).contains(&response.status()) {
        return None;
    }
    let location = response.header("Location")?;
    url::Url::parse(response.get_url()).ok()?.join(location).ok().map(String::from)
}
