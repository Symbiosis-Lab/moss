//! Which hosts one plugin request may reach. The transport checks the first
//! address and every redirect target, so a declared host cannot bounce a
//! request elsewhere. A plugin with no `network` declaration reaches nothing.
//! An entry may carry a port; a loopback entry must, since every program on
//! the computer listens there, so one without a port reaches nothing.

use crate::plugins::types::NetworkDecl;

/// The hosts a request may reach, or why it may reach none.
#[derive(Debug, Clone)]
pub struct AllowedHosts(Allow);

#[derive(Debug, Clone)]
enum Allow {
    Hosts { plugin: String, patterns: Vec<Pattern> },
    Refused(Refusal),
    /// The desktop app's panel commands, which do not yet carry their
    /// plugin's declarations. Never built for engine code.
    Unchecked,
}

/// Why a plugin reaches no host. Worded at `check`, which knows the host.
#[derive(Debug, Clone)]
pub enum Refusal {
    /// The plugin has no `network` declaration.
    Undeclared(String),
    /// moss could not tell which plugin called.
    Unidentified,
    /// The plugin is serving a build's run.
    Building(String),
    /// The plugin's code is still running after the person's run ended.
    OutsideARun(String),
}

impl AllowedHosts {
    /// What `plugin` may reach: its snapshotted declaration, the host of each
    /// named setting's current value (`setting`), and the site's own host.
    pub fn for_plugin(
        plugin: &str,
        network: Option<&NetworkDecl>,
        setting: impl Fn(&str) -> Option<String>,
        site_host: impl FnOnce() -> Option<String>,
    ) -> Self {
        let Some(network) = network else {
            return Self::refused(Refusal::Undeclared(plugin.to_string()));
        };
        let mut patterns: Vec<Pattern> = network.hosts.iter().map(|h| entry(h)).collect();
        patterns.extend(network.setting_hosts.iter().filter_map(|key| setting(key)).filter_map(|v| typed(&v)));
        if network.site {
            patterns.extend(site_host().and_then(|h| typed(&h)));
        }
        patterns.retain(|p| p.port.is_some() || !loopback(&p.host));
        Self(Allow::Hosts { plugin: plugin.to_string(), patterns })
    }

    /// Refuses everything, for `why`.
    pub fn refused(why: Refusal) -> Self {
        Self(Allow::Refused(why))
    }

    /// No check: the desktop app's panel commands until they carry their
    /// plugin's declarations.
    pub fn unchecked_app_panel() -> Self {
        Self(Allow::Unchecked)
    }

    /// The plugin these hosts belong to, when moss knows it.
    pub(crate) fn plugin(&self) -> Option<&str> {
        match &self.0 {
            Allow::Hosts { plugin, .. } => Some(plugin),
            _ => None,
        }
    }

    /// Refuse `url` unless its host is allowed.
    pub(crate) fn check(&self, url: &url::Url) -> Result<(), String> {
        match &self.0 {
            Allow::Unchecked => Ok(()),
            Allow::Refused(why) => {
                let host = url.host_str().unwrap_or_default();
                let outside = |plugin: &str, when: &str| format!("{plugin} tried to reach {host} {when}; plugins reach the network only when you Sync or Publish.");
                Err(match why {
                    Refusal::Undeclared(plugin) => format!("{plugin} does not list the hosts it contacts, so it cannot reach the network. Update it in Plugins."),
                    Refusal::Unidentified => unidentified(&format!("reach {host}")),
                    Refusal::Building(plugin) => outside(plugin, "during a build"),
                    Refusal::OutsideARun(plugin) => outside(plugin, "outside a run you started"),
                })
            }
            Allow::Hosts { plugin, patterns } => {
                let host = normalize(url.host_str().unwrap_or_default());
                let host = host.trim_start_matches('[').trim_end_matches(']');
                let port = url.port_or_known_default();
                if patterns.iter().any(|p| matches(&p.host, host) && p.port.is_none_or(|want| Some(want) == port)) {
                    Ok(())
                } else {
                    Err(format!("{plugin} tried to reach {host}, which it does not list in its manifest."))
                }
            }
        }
    }
}

/// The refusal for a call moss cannot tie to a plugin, which `tried` something.
pub(crate) fn unidentified(tried: &str) -> String {
    format!("a plugin moss could not identify tried to {tried}; refusing")
}

#[derive(Debug, Clone)]
struct Pattern {
    host: String,
    /// `None`: any port.
    port: Option<u16>,
}

fn normalize(host: &str) -> String {
    host.trim().trim_end_matches('.').to_ascii_lowercase()
}

/// A manifest entry: a host pattern, optionally `:<port>` (`[::1]:5001`).
fn entry(raw: &str) -> Pattern {
    let (host, port) = crate::system::proxy::split_entry_port(raw.trim());
    Pattern { host: normalize(host), port }
}

/// An address the person typed, with or without a scheme. A loopback address
/// keeps its port, the scheme's default when none was typed.
fn typed(value: &str) -> Option<Pattern> {
    let value = value.trim();
    let parsed = url::Url::parse(value).ok().filter(|u| u.host_str().is_some())
        .or_else(|| url::Url::parse(&format!("https://{value}")).ok())?;
    let host = parsed.host_str().map(normalize).filter(|h| !h.is_empty())?;
    let host = host.trim_start_matches('[').trim_end_matches(']').to_string();
    let port = loopback(&host).then(|| parsed.port_or_known_default()).flatten();
    Some(Pattern { host, port })
}

/// Whether `pattern` names only loopback addresses; `*.localhost` does.
fn loopback(pattern: &str) -> bool {
    let host = pattern.strip_prefix("*.").unwrap_or(pattern);
    let host = if host.contains(':') { format!("[{host}]") } else { host.to_string() };
    url::Url::parse(&format!("http://{host}/")).ok().is_some_and(|u| u.host().is_some_and(|h| crate::system::proxy::is_loopback(&h)))
}

/// `*.example.com` matches any name below `example.com`, never the domain
/// itself; any other pattern matches only itself.
fn matches(pattern: &str, host: &str) -> bool {
    match pattern.strip_prefix("*.") {
        Some(domain) => host.len() > domain.len() + 1 && host.ends_with(domain) && host.as_bytes()[host.len() - domain.len() - 1] == b'.',
        None => !pattern.is_empty() && pattern == host,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decl(hosts: &[&str], setting_hosts: &[&str], site: bool) -> NetworkDecl {
        NetworkDecl { hosts: hosts.iter().map(|s| s.to_string()).collect(), setting_hosts: setting_hosts.iter().map(|s| s.to_string()).collect(), site }
    }
    fn url(s: &str) -> url::Url {
        s.parse().unwrap()
    }

    #[test]
    fn a_wildcard_reaches_names_below_its_domain_only() {
        assert!(matches("*.example.com", "a.example.com"));
        assert!(matches("*.example.com", "a.b.example.com"));
        assert!(!matches("*.example.com", "example.com"));
        assert!(!matches("*.example.com", "badexample.com"));
        assert!(!matches("example.com", "a.example.com"));
    }

    #[test]
    fn declared_hosts_settings_and_the_site_are_allowed_and_nothing_else() {
        let allowed = AllowedHosts::for_plugin(
            "social",
            Some(&decl(&["api.example.com"], &["server"], true)),
            |key| (key == "server").then(|| "https://Social.Example.org/".to_string()),
            || Some("blog.example.net".to_string()),
        );
        for ok in ["https://api.example.com/v1", "http://api.example.com:8080/", "https://social.example.org/api", "https://blog.example.net/"] {
            assert!(allowed.check(&url(ok)).is_ok(), "{ok}");
        }
        let err = allowed.check(&url("https://evil.example/x")).unwrap_err();
        assert_eq!(err, "social tried to reach evil.example, which it does not list in its manifest.");
    }

    #[test]
    fn a_setting_typed_without_a_scheme_still_names_its_host() {
        let typed = |v: &str| typed(v).map(|p| (p.host, p.port));
        assert_eq!(typed("mastodon.example"), Some(("mastodon.example".into(), None)));
        assert_eq!(typed("https://Mastodon.Example:8443/@me"), Some(("mastodon.example".into(), None)));
        assert_eq!(typed("http://127.0.0.1:5001"), Some(("127.0.0.1".into(), Some(5001))));
        assert_eq!(typed("localhost"), Some(("localhost".into(), Some(443))));
        assert_eq!(typed("http://[::1]:5001/"), Some(("::1".into(), Some(5001))));
    }

    #[test]
    fn a_loopback_entry_reaches_only_the_port_it_names() {
        let allowed = AllowedHosts::for_plugin(
            "node",
            Some(&decl(&["127.0.0.1", "127.0.0.1:5001", "localhost", "*.ipfs.localhost:8080", "[::1]:5001", "api.example.com:8443", "0.0.0.0", "::", "::ffff:7f00:1"], &[], false)),
            |_| None,
            || None,
        );
        for ok in ["http://127.0.0.1:5001/api/v0/add", "http://bafy.ipfs.localhost:8080/", "http://[::1]:5001/", "https://api.example.com:8443/"] {
            assert!(allowed.check(&url(ok)).is_ok(), "{ok}");
        }
        for refused in ["http://127.0.0.1:22/", "http://127.0.0.1/", "http://localhost:5001/", "http://bafy.ipfs.localhost:9000/", "https://api.example.com/", "http://0.0.0.0:22/", "http://[::]:22/", "http://[::ffff:127.0.0.1]:22/"] {
            assert!(allowed.check(&url(refused)).is_err(), "{refused}");
        }
    }

    #[test]
    fn a_plugin_with_no_declaration_reaches_nothing() {
        let none = AllowedHosts::for_plugin("old", None, |_| None, || None);
        assert!(none.check(&url("https://api.example.com/")).unwrap_err().contains("does not list the hosts"));
    }
}
