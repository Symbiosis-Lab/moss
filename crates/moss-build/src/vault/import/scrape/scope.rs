//! URL scope checking for the website scraper
//!
//! Determines whether a URL is within the crawl scope based on the starting URL.

use url::Url;

/// Represents the scope of URLs to crawl
#[derive(Debug, Clone)]
pub struct UrlScope {
    /// The base URL (scheme + host)
    base_url: Url,
    /// The path prefix that URLs must start with
    path_prefix: String,
}

/// Error type for scope creation
#[derive(Debug, thiserror::Error)]
pub enum ScopeError {
    #[error("Invalid URL: {0}")]
    InvalidUrl(#[from] url::ParseError),
    #[error("URL must have a scheme and host")]
    MissingSchemeOrHost,
}

impl UrlScope {
    /// Create a new URL scope from a starting URL
    ///
    /// The scope will include:
    /// - The exact URL
    /// - Any URLs with the same host and a path that starts with the starting URL's path
    pub fn new(starting_url: &str) -> Result<Self, ScopeError> {
        if starting_url.is_empty() {
            return Err(ScopeError::InvalidUrl(url::ParseError::EmptyHost));
        }

        let url = Url::parse(starting_url)?;

        if url.host().is_none() {
            return Err(ScopeError::MissingSchemeOrHost);
        }

        // Normalize the path prefix (remove trailing slash for comparison)
        let path = url.path();
        let path_prefix = if path.is_empty() {
            "/".to_string()
        } else {
            trim_one_trailing_slash(path).to_string()
        };

        // Create base URL (scheme + host + port)
        let mut base_url = url.clone();
        base_url.set_path("");
        base_url.set_query(None);
        base_url.set_fragment(None);

        Ok(Self {
            base_url,
            path_prefix,
        })
    }

    /// Get the path prefix
    pub fn path_prefix(&self) -> &str {
        &self.path_prefix
    }
}

/// `path` with one trailing `/` removed, leaving the bare root `"/"` alone.
fn trim_one_trailing_slash(path: &str) -> &str {
    match path.strip_suffix('/') {
        Some("") | None => path,
        Some(trimmed) => trimmed,
    }
}

/// Check if a URL is within the given scope
pub fn is_within_scope(scope: &UrlScope, url_str: &str) -> bool {
    let url = match Url::parse(url_str) {
        Ok(u) => u,
        Err(_) => return false,
    };

    // Check host matches (including scheme)
    if url.scheme() != scope.base_url.scheme() {
        return false;
    }

    if url.host() != scope.base_url.host() {
        return false;
    }

    if url.port() != scope.base_url.port() {
        return false;
    }

    // Check path starts with the scope's path prefix
    let path = url.path();

    // Handle root scope specially
    if scope.path_prefix == "/" {
        return true;
    }

    // Normalize the path for comparison
    let normalized_path = trim_one_trailing_slash(path);

    // Path must either match exactly or be a subpath
    normalized_path == scope.path_prefix
        || normalized_path.starts_with(&format!("{}/", scope.path_prefix))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scope_creation() {
        let scope = UrlScope::new("https://example.com/blog/").unwrap();
        assert_eq!(scope.path_prefix(), "/blog");
    }

    #[test]
    fn test_scope_root() {
        let scope = UrlScope::new("https://example.com/").unwrap();
        assert_eq!(scope.path_prefix(), "/");
    }

    #[test]
    fn test_scope_includes_exact_url() {
        let scope = UrlScope::new("https://example.com/blog/").unwrap();
        assert!(is_within_scope(&scope, "https://example.com/blog/"));
    }
    
    #[test]
    fn test_scope_includes_subpath() {
        let scope = UrlScope::new("https://example.com/blog/").unwrap();
        assert!(is_within_scope(&scope, "https://example.com/blog/post-1"));
        assert!(is_within_scope(&scope, "https://example.com/blog/2024/intro"));
    }
    
    #[test]
    fn test_scope_excludes_sibling_paths() {
        let scope = UrlScope::new("https://example.com/blog/").unwrap();
        assert!(!is_within_scope(&scope, "https://example.com/about"));
        assert!(!is_within_scope(&scope, "https://example.com/contact/"));
    }
    
    #[test]
    fn test_scope_excludes_external_domains() {
        let scope = UrlScope::new("https://example.com/blog/").unwrap();
        assert!(!is_within_scope(&scope, "https://other.com/blog/"));
        assert!(!is_within_scope(&scope, "https://subdomain.example.com/blog/"));
    }
    
    #[test]
    fn test_scope_handles_trailing_slash_normalization() {
        // /blog and /blog/ should be treated the same
        let scope_with_slash = UrlScope::new("https://example.com/blog/").unwrap();
        let scope_without_slash = UrlScope::new("https://example.com/blog").unwrap();
    
        assert!(is_within_scope(&scope_with_slash, "https://example.com/blog"));
        assert!(is_within_scope(&scope_with_slash, "https://example.com/blog/"));
        assert!(is_within_scope(&scope_without_slash, "https://example.com/blog"));
        assert!(is_within_scope(&scope_without_slash, "https://example.com/blog/"));
    }
    
    #[test]
    fn test_scope_root_url_includes_all_paths() {
        let scope = UrlScope::new("https://example.com/").unwrap();
        assert!(is_within_scope(&scope, "https://example.com/"));
        assert!(is_within_scope(&scope, "https://example.com/blog"));
        assert!(is_within_scope(&scope, "https://example.com/about/team"));
    }
    
    #[test]
    fn test_scope_rejects_invalid_url() {
        assert!(UrlScope::new("not-a-url").is_err());
        assert!(UrlScope::new("").is_err());
    }
    
    #[test]
    fn test_scope_handles_query_strings() {
        let scope = UrlScope::new("https://example.com/blog/").unwrap();
        // URLs with query strings within scope should be included
        assert!(is_within_scope(&scope, "https://example.com/blog/post?page=2"));
    }
    
    #[test]
    fn test_scope_handles_fragments() {
        let scope = UrlScope::new("https://example.com/blog/").unwrap();
        // URLs with fragments within scope should be included
        assert!(is_within_scope(&scope, "https://example.com/blog/post#section"));
    }
}
