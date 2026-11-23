//! Signing in through the person's own browser, the way RFC 8252 asks native
//! apps to: the channel's sign-in page opens in the default browser, where the
//! person is usually signed in already, and the provider sends the browser back
//! to a one-request listener on `127.0.0.1`.
//!
//! [`start`] binds the listener and returns the redirect address; [`wait`] opens
//! the sign-in page and returns the query the provider sent back. The plugin
//! checks `state` and exchanges the code: moss cannot know either.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::Duration;

use tokio::sync::{mpsc, oneshot};

/// How long a sign-in may take before moss stops listening.
pub const SIGN_IN_LIMIT: Duration = Duration::from_secs(10 * 60);

/// Where the provider sends the browser back.
pub const CALLBACK_PATH: &str = "/callback";

/// What `wait` ends with when the person presses Cancel. moss-api turns
/// exactly this text into `SignInCancelled`.
pub const CANCELLED: &str = "sign-in cancelled";

/// What `wait` ends with at [`SIGN_IN_LIMIT`].
pub const TOO_LONG: &str = "the sign-in took longer than ten minutes";

/// An address moss may hand to the system browser: `https`, or plain `http`
/// to a loopback literal (a local test server). Anything else — `file:`,
/// `javascript:`, an app's custom scheme — would let a plugin open more than a
/// web page. The refusal names only the scheme: the address can carry `state`.
pub fn check_browser_address(address: &str) -> Result<url::Url, String> {
    let Ok(url) = url::Url::parse(address) else { return Err("moss opens only https addresses in the browser".to_string()) };
    let loopback = match url.host() {
        Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
        _ => false,
    };
    match url.scheme() {
        "https" if url.has_host() => Ok(url),
        "http" if loopback => Ok(url),
        scheme => Err(format!("moss opens only https addresses in the browser, not {scheme}:")),
    }
}

type Query = Vec<(String, String)>;

/// A plugin's one sign-in. Dropping it ends the sign-in: its listener task
/// sees `_stop` close, tells a waiter it was cancelled, and stops listening.
struct SignIn {
    token: u64,
    _stop: oneshot::Sender<()>,
    /// Until [`wait`] takes it.
    reply: Option<oneshot::Receiver<Result<Query, String>>>,
}

fn sign_ins() -> MutexGuard<'static, HashMap<String, SignIn>> {
    static SIGN_INS: OnceLock<Mutex<HashMap<String, SignIn>>> = OnceLock::new();
    SIGN_INS.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner())
}

/// Bind a listener on `127.0.0.1` at a port the system picks, for `plugin`,
/// ending any sign-in it had started. It answers the first request on
/// [`CALLBACK_PATH`] that carries `code` or `error` and stops; it stops anyway
/// after [`SIGN_IN_LIMIT`]. The path is fixed because a provider matches a
/// loopback redirect exactly except for its port (RFC 8252 §7.3), and atproto
/// clients publish theirs; the plugin's `state` check, not the path, tells its
/// reply from anything else on this computer.
pub async fn start(plugin: &str) -> Result<String, String> {
    static TOKENS: AtomicU64 = AtomicU64::new(0);
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await.map_err(|e| format!("could not listen for the sign-in: {e}"))?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let token = TOKENS.fetch_add(1, Ordering::Relaxed);
    let (got_tx, mut got) = mpsc::channel::<Query>(1);
    let (mut reply_tx, reply) = oneshot::channel();
    let (stop_tx, stop) = oneshot::channel::<()>();

    let plugin_name = plugin.to_string();
    let router = axum::Router::new().route(
        CALLBACK_PATH,
        axum::routing::get(move |axum::extract::RawQuery(query): axum::extract::RawQuery| {
            let (got_tx, plugin) = (got_tx.clone(), plugin_name.clone());
            async move {
                use axum::response::IntoResponse;
                let params: Query = url::form_urlencoded::parse(query.unwrap_or_default().as_bytes()).into_owned().collect();
                if !params.iter().any(|(k, _)| k == "code" || k == "error") {
                    log::info!(target: "plugin", "{plugin}: a request reached the sign-in redirect without a code or an error");
                    return axum::http::StatusCode::NOT_FOUND.into_response();
                }
                let _ = got_tx.try_send(params);
                ([(axum::http::header::CACHE_CONTROL, "no-store")], axum::response::Html(PAGE)).into_response()
            }
        }),
    );
    let plugin_name = plugin.to_string();
    tokio::spawn(async move {
        let ended = async move {
            // One fixed line per ending, its own call site each, so a support
            // log says whether the browser ever came back.
            let outcome = tokio::select! {
                Some(query) = got.recv() => {
                    if query.iter().any(|(k, _)| k == "code") {
                        log::info!(target: "plugin", "{plugin_name}: the sign-in redirect arrived with a code");
                    } else {
                        log::info!(target: "plugin", "{plugin_name}: the sign-in redirect arrived with an error");
                    }
                    Ok(query)
                }
                _ = stop => {
                    log::info!(target: "plugin", "{plugin_name}: the sign-in ended before its redirect arrived");
                    Err(CANCELLED.to_string())
                }
                // The waiter gave up: a refused address, a browser that would not open.
                _ = reply_tx.closed() => {
                    log::info!(target: "plugin", "{plugin_name}: the sign-in stopped waiting before its redirect arrived");
                    Err(CANCELLED.to_string())
                }
                _ = tokio::time::sleep(SIGN_IN_LIMIT) => {
                    log::warn!(target: "plugin", "{plugin_name}: no sign-in redirect arrived in ten minutes");
                    Err(TOO_LONG.to_string())
                }
            };
            let mut map = sign_ins();
            if map.get(&plugin_name).is_some_and(|s| s.token == token) {
                map.remove(&plugin_name);
            }
            drop(map);
            let _ = reply_tx.send(outcome);
        };
        let _ = axum::serve(listener, router).with_graceful_shutdown(ended).await;
    });

    sign_ins().insert(plugin.to_string(), SignIn { token, _stop: stop_tx, reply: Some(reply) });
    Ok(format!("http://127.0.0.1:{port}{CALLBACK_PATH}"))
}

/// Open `address` with `open` and wait for the provider's redirect to the
/// sign-in `plugin` started. Returns the redirect's query, `error` included:
/// what a refusal means is the plugin's to say. Any error here ends the
/// sign-in.
pub async fn wait<F, Fut>(plugin: &str, address: &str, open: F) -> Result<Query, String>
where
    F: FnOnce(String) -> Fut,
    Fut: std::future::Future<Output = Result<(), String>>,
{
    let reply = sign_ins().get_mut(plugin).and_then(|s| s.reply.take()).ok_or_else(|| format!("{plugin} has no sign-in to wait on"))?;
    let url = check_browser_address(address)?;
    open(url.into()).await?;
    reply.await.unwrap_or_else(|_| Err("the sign-in stopped listening".to_string()))
}

/// End `plugin`'s sign-in if it is waiting on the browser, answering whether
/// it was. The browser tells moss nothing when its tab closes, so a press on
/// Sign in while one waits is the person's only sign that they left it.
pub fn end_wait(plugin: &str) -> bool {
    let mut map = sign_ins();
    let waiting = map.get(plugin).is_some_and(|s| s.reply.is_none());
    if waiting {
        map.remove(plugin);
    }
    waiting
}

/// The page the browser lands on, before the plugin has checked the reply.
/// moss comes forward on its own; the tab is the one thing left for the person.
const PAGE: &str = "<!doctype html><html><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width\"><title>moss</title>\
<style>body{font:16px/1.5 -apple-system,system-ui,sans-serif;margin:0;min-height:100vh;display:grid;place-items:center;background:#faf9f6;color:#222}\
@media (prefers-color-scheme:dark){body{background:#1c1c1c;color:#eee}}</style></head><body><p>You can close this tab.</p></body></html>";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_browser_opens_only_web_pages() {
        assert!(check_browser_address("https://mastodon.social/oauth/authorize?x=1").is_ok());
        assert!(check_browser_address("http://127.0.0.1:8080/oauth").is_ok());
        assert!(check_browser_address("http://[::1]:8080/oauth").is_ok());
        for refused in ["file:///etc/passwd", "javascript:alert(1)", "moss://open", "http://example.com/", "http://localhost:80/", "ftp://example.com/", "not a url", "https://"] {
            assert!(check_browser_address(refused).is_err(), "{refused} must be refused");
        }
    }

    #[test]
    fn a_refusal_names_the_scheme_not_the_address() {
        let err = check_browser_address("moss://open?state=secret-state").unwrap_err();
        assert!(err.ends_with("not moss:"), "{err}");
        assert!(!err.contains("secret-state"), "{err}");
    }

    async fn get(url: &str) -> (u16, String) {
        // The listener is on this computer; a proxy from the environment must not carry the request away.
        let client = crate::system::proxy_reqwest::client_builder().no_proxy().build().unwrap();
        let r = client.get(url).send().await.expect("listener answers");
        let status = r.status().as_u16();
        (status, r.text().await.unwrap_or_default())
    }

    /// Whether the listener at `redirect` is gone, within a second.
    async fn stops_listening(redirect: &str) -> bool {
        let addr = redirect.trim_start_matches("http://").split('/').next().unwrap().to_string();
        for _ in 0..50 {
            if tokio::net::TcpStream::connect(&addr).await.is_err() {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        false
    }

    use crate::build::cache::tests::{install_capture, logged_count};

    #[tokio::test]
    async fn browser_sign_in_brings_back_the_redirect_with_a_code() {
        install_capture();
        let redirect = start("browser-sign-in-reply").await.unwrap();
        assert!(redirect.starts_with("http://127.0.0.1:") && redirect.ends_with("/callback"), "{redirect}");
        let (browser_tx, browser) = oneshot::channel();
        let got = wait("browser-sign-in-reply", "https://example.com/authorize", {
            let redirect = redirect.clone();
            move |opened| async move {
                assert_eq!(opened, "https://example.com/authorize");
                let base = redirect.rsplit_once('/').unwrap().0.to_string();
                let _ = browser_tx.send(tokio::spawn(async move {
                    let wrong_path = get(&format!("{base}/wrong-path?code=x")).await.0;
                    let no_code = get(&format!("{redirect}?state=s")).await.0;
                    (wrong_path, no_code, get(&format!("{redirect}?code=abc&state=s%201")).await)
                }));
                Ok(())
            }
        })
        .await
        .unwrap();
        assert_eq!(got, vec![("code".into(), "abc".into()), ("state".into(), "s 1".into())]);
        let (wrong_path, no_code, (status, body)) = browser.await.unwrap().await.unwrap();
        assert_eq!((wrong_path, no_code, status), (404, 404, 200), "only a /callback with a code or an error is answered");
        assert!(body.contains("<p>You can close this tab.</p>"), "{body}");
        assert!(stops_listening(&redirect).await);
        assert_eq!(logged_count(log::Level::Info, "browser-sign-in-reply: a request reached the sign-in redirect without a code or an error"), 1);
        assert_eq!(logged_count(log::Level::Info, "browser-sign-in-reply: the sign-in redirect arrived with a code"), 1);
    }

    #[tokio::test]
    async fn a_redirect_with_an_error_is_logged_as_one() {
        install_capture();
        let redirect = start("browser-sign-in-denied").await.unwrap();
        let got = wait("browser-sign-in-denied", "https://example.com/", move |_| async move {
            tokio::spawn(async move { get(&format!("{redirect}?error=access_denied")).await });
            Ok(())
        })
        .await
        .unwrap();
        assert_eq!(got, vec![("error".into(), "access_denied".into())]);
        assert_eq!(logged_count(log::Level::Info, "browser-sign-in-denied: the sign-in redirect arrived with an error"), 1);
    }

    #[tokio::test]
    async fn browser_sign_in_ends_at_a_refused_address() {
        install_capture();
        let redirect = start("browser-sign-in-refused").await.unwrap();
        let refused = wait("browser-sign-in-refused", "file:///etc/passwd", |_| async { panic!("must not open") }).await;
        assert!(refused.is_err());
        assert!(stops_listening(&redirect).await, "no listener may be left up");
        assert_eq!(logged_count(log::Level::Info, "browser-sign-in-refused: the sign-in stopped waiting before its redirect arrived"), 1);
    }

    async fn soon(waiting: tokio::task::JoinHandle<Result<Query, String>>) -> Result<Query, String> {
        tokio::time::timeout(Duration::from_secs(5), waiting).await.expect("the wait ends").unwrap()
    }

    #[tokio::test]
    async fn browser_sign_in_cancel_ends_the_wait() {
        install_capture();
        start("browser-sign-in-cancel").await.unwrap();
        assert!(!end_wait("browser-sign-in-cancel"), "a sign-in not yet in the browser is not ended");
        let waiting = tokio::spawn(wait("browser-sign-in-cancel", "https://example.com/", |_| async { Ok(()) }));
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(end_wait("browser-sign-in-cancel"));
        assert_eq!(soon(waiting).await, Err(CANCELLED.to_string()));
        assert_eq!(logged_count(log::Level::Info, "browser-sign-in-cancel: the sign-in ended before its redirect arrived"), 1);
    }

    /// A second start ends the first sign-in, waited on or not, and the reply
    /// comes from the second one's listener.
    #[tokio::test]
    async fn browser_sign_in_started_again_replaces_the_first() {
        let first = start("browser-sign-in-again").await.unwrap();
        let waiting = tokio::spawn(wait("browser-sign-in-again", "https://example.com/", |_| async { Ok(()) }));
        tokio::time::sleep(Duration::from_millis(50)).await;
        let second = start("browser-sign-in-again").await.unwrap();
        assert_eq!(soon(waiting).await, Err(CANCELLED.to_string()));
        assert!(stops_listening(&first).await);

        let third = start("browser-sign-in-again").await.unwrap();
        assert!(stops_listening(&second).await, "a started sign-in nobody waited on ends too");
        let got = wait("browser-sign-in-again", "https://example.com/", move |_| async move {
            tokio::spawn(async move { get(&format!("{third}?code=c3")).await });
            Ok(())
        })
        .await
        .unwrap();
        assert_eq!(got, vec![("code".into(), "c3".into())]);
    }

    /// The listener's own deadline ends the wait with its message, and the
    /// sign-in is gone afterwards.
    #[tokio::test(start_paused = true)]
    async fn browser_sign_in_stops_at_the_limit() {
        install_capture();
        start("browser-sign-in-limit").await.unwrap();
        let outcome = tokio::time::timeout(SIGN_IN_LIMIT * 2, wait("browser-sign-in-limit", "https://example.com/", |_| async { Ok(()) })).await;
        assert_eq!(outcome, Ok(Err(TOO_LONG.to_string())));
        assert!(!sign_ins().contains_key("browser-sign-in-limit"));
        assert_eq!(logged_count(log::Level::Warn, "browser-sign-in-limit: no sign-in redirect arrived in ten minutes"), 1);
    }
}
