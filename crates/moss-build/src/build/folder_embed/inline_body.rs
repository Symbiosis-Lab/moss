//! A listed page's rendered body, as another page draws it.
//!
//! A page's HTML is written for its own address. A listing that draws it
//! elsewhere drops the title heading the listing draws itself and rewrites the
//! relative references so they resolve from the listing's directory.

/// Directory of a served page, as segments: `board/sub/index.html` is
/// `["board", "sub"]`.
pub(crate) fn dir_segments(url_path: &str) -> Vec<String> {
    let dir = url_path.rsplit_once('/').map_or("", |(dir, _)| dir);
    dir.split('/').filter(|s| !s.is_empty()).map(str::to_string).collect()
}

/// The path from directory `from` to `to_segments`, `../` as needed.
pub(crate) fn relative(from: &[String], to: &[String]) -> String {
    let common = from.iter().zip(to).take_while(|(a, b)| a == b).count();
    let mut parts: Vec<&str> = vec![".."; from.len() - common];
    parts.extend(to[common..].iter().map(String::as_str));
    parts.join("/")
}

/// Rewrite the relative references in a page body so they resolve from
/// `to_dir` instead of from the page `page_url_path`.
pub(crate) fn rebase_html(html: &str, page_url_path: &str, to_dir: &[String]) -> String {
    static ATTR: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r#"((?:src|srcset|poster|href)=")([^"]*)""#).unwrap());
    let page_dir = dir_segments(page_url_path);
    let rebase_one = |url: &str| -> String {
        let skip = url.is_empty()
            || url.starts_with('/')
            || url.starts_with('#')
            || url.contains("://")
            || url.split(':').next().is_some_and(|scheme| url.contains(':') && scheme.chars().all(|c| c.is_ascii_alphabetic()));
        if skip {
            return url.to_string();
        }
        let (path, suffix) = match url.find(['?', '#']) {
            Some(i) => url.split_at(i),
            None => (url, ""),
        };
        let mut segments = page_dir.clone();
        for part in path.split('/') {
            match part {
                "" | "." => {}
                ".." => {
                    segments.pop();
                }
                other => segments.push(other.to_string()),
            }
        }
        let mut rebased = relative(to_dir, &segments);
        if path.ends_with('/') && !rebased.is_empty() {
            rebased.push('/');
        }
        if rebased.is_empty() {
            rebased.push_str("./");
        }
        format!("{rebased}{suffix}")
    };
    ATTR.replace_all(html, |caps: &regex::Captures| {
        let value = if caps[1].starts_with("srcset") {
            caps[2]
                .split(", ")
                .map(|candidate| match candidate.split_once(' ') {
                    Some((url, descriptor)) => format!("{} {descriptor}", rebase_one(url)),
                    None => rebase_one(candidate),
                })
                .collect::<Vec<_>>()
                .join(", ")
        } else {
            rebase_one(&caps[2])
        };
        format!("{}{}\"", &caps[1], value)
    })
    .into_owned()
}

/// The body without the heading moss draws from the page's title: the record
/// carries the title as a field, and a plugin that lists the page draws its own.
pub(crate) fn without_title(html: &str) -> String {
    static HEADING: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r#"(?s)<h1 class="moss-article-title"[^>]*>.*?</h1>\n?"#).unwrap());
    // The editor's source-line markers are not part of a published page either.
    crate::build::ship::strip_source_annotations(&HEADING.replacen(html, 1, "")).into_owned()
}

/// `doc`'s body as a page in directory `to_dir` draws it.
pub(crate) fn listed_body(doc: &crate::build::types::ParsedDocument, to_dir: &[String]) -> String {
    rebase_html(&without_title(&doc.html_content), &doc.url_path, to_dir)
}

/// The ids in one document (a page, or a feed), so each body drawn into it
/// keeps its ids apart from every other body's and from the document's own.
#[derive(Default)]
pub(crate) struct IdScopes {
    used: std::collections::HashSet<String>,
    taken_ids: std::collections::HashSet<String>,
}

impl IdScopes {
    /// Scopes for a document whose own HTML already defines the ids in `html`.
    pub(crate) fn around(html: &str) -> Self {
        Self { used: Default::default(), taken_ids: ids_in(html).into_iter().map(str::to_string).collect() }
    }

    /// `body`, the body of the page at `url_path`, with its ids under a prefix
    /// no other body here has and that makes none of its ids one the document
    /// already holds.
    ///
    /// moss slugs keep CJK as written, and pretty URLs all end in `index.html`,
    /// so the prefix is the slugified directory part; where that is taken or
    /// would collide (`posts/a` holding `b-intro` against `posts/a-b` holding
    /// `intro`, or the root, which slugifies to nothing) the body's position
    /// decides.
    pub(crate) fn scope(&mut self, body: &str, url_path: &str, index: usize) -> String {
        let ids = ids_in(body);
        let stem = url_path.strip_suffix("index.html").or_else(|| url_path.strip_suffix(".html")).unwrap_or(url_path);
        let mut scope = moss_core::slug::slugify_path_segments(stem).replace('/', "-");
        let mut ordinal = index + 1;
        let clashes = |scope: &str| ids.iter().any(|id| self.taken_ids.contains(&format!("{scope}-{id}")));
        while scope.is_empty() || self.used.contains(&scope) || clashes(&scope) {
            scope = format!("item-{ordinal}");
            ordinal += 1;
        }
        self.taken_ids.extend(ids.iter().map(|id| format!("{scope}-{id}")));
        self.used.insert(scope.clone());
        scope_ids(body, &scope)
    }
}

fn ids_in(html: &str) -> std::collections::HashSet<&str> {
    static ID: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| regex::Regex::new(r#"\sid="([^"]+)""#).unwrap());
    ID.captures_iter(html).filter_map(|c| c.get(1)).map(|m| m.as_str()).collect()
}

/// Every id `html` defines, and every reference to one of them, under `scope`:
/// fragment links, `xlink:href`, SVG `url(#…)`, a label's `for` and the
/// `aria-*` attributes that name ids (each token of a list). A reference to an
/// id the body does not define is left alone.
fn scope_ids(html: &str, scope: &str) -> String {
    static REF: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r##"(\s(?:id|for|aria-labelledby|aria-describedby|aria-controls|aria-owns|aria-details|aria-errormessage|aria-flowto|aria-activedescendant)=")([^"]*)"|((?:\s(?:xlink:)?href="|url\('?)#)([^"')\s]+)"##).unwrap()
    });
    let ids = ids_in(html);
    if ids.is_empty() {
        return html.to_string();
    }
    REF.replace_all(html, |c: &regex::Captures| match (c.get(1), c.get(2), c.get(3), c.get(4)) {
        (Some(attr), Some(value), _, _) => {
            let is_id = attr.as_str().trim_start() == "id=\"";
            let tokens: Vec<String> = value
                .as_str()
                .split(' ')
                .map(|t| if !t.is_empty() && (is_id || ids.contains(t)) { format!("{scope}-{t}") } else { t.to_string() })
                .collect();
            format!("{}{}\"", attr.as_str(), tokens.join(" "))
        }
        (_, _, Some(lead), Some(name)) if ids.contains(fragment_id(name.as_str()).as_ref()) => format!("{}{scope}-{}", lead.as_str(), name.as_str()),
        _ => c[0].to_string(),
    })
    .into_owned()
}

/// The id a fragment names: a browser percent-decodes it before matching.
fn fragment_id(fragment: &str) -> std::borrow::Cow<'_, str> {
    percent_encoding::percent_decode_str(fragment).decode_utf8_lossy()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn segs(path: &str) -> Vec<String> {
        path.split('/').filter(|s| !s.is_empty()).map(str::to_string).collect()
    }

    #[test]
    fn a_body_is_rebased_from_its_page_to_the_listing() {
        let html = r##"<img src="../pic.jpg"> <a href="../../beta/#x">b</a> <a href="/abs/">a</a> <a href="https://e.com/">e</a> <a href="#top">t</a>"##;
        let out = rebase_html(html, "board/sub/alpha/index.html", &segs("board"));
        assert!(out.contains(r#"src="sub/pic.jpg""#), "{out}");
        assert!(out.contains(r#"href="beta/#x""#), "{out}");
        assert!(out.contains(r#"href="/abs/""#) && out.contains(r#"href="https://e.com/""#) && out.contains(r##"href="#top""##), "{out}");
    }

    #[test]
    fn urls_are_relative_to_the_listing_page() {
        assert_eq!(relative(&segs("board"), &segs("board/sub")), "sub");
        assert_eq!(relative(&segs("board"), &segs("other/x")), "../other/x");
        assert_eq!(relative(&segs(""), &segs("board")), "board");
    }

    #[test]
    fn every_reference_to_a_scoped_id_follows_it() {
        let html = concat!(
            r##"<h2 id="intro">I</h2><a href="#intro">i</a><a href="#elsewhere">e</a>"##,
            r##"<label for="email">E</label><input id="email" aria-describedby="hint other"><p id="hint">h</p>"##,
            r##"<svg aria-labelledby="t"><title id="t">T</title><defs><linearGradient id="g"/></defs><rect fill="url(#g)" style="fill:url('#g')"/><use xlink:href="#g"/></svg>"##,
        );
        let out = scope_ids(html, "p");
        for expected in [
            r#"id="p-intro""#, r##"href="#p-intro""##, r##"href="#elsewhere""##,
            r#"for="p-email""#, r#"id="p-email""#, r#"aria-describedby="p-hint other""#,
            r#"aria-labelledby="p-t""#, r#"fill="url(#p-g)""#, "url('#p-g')", r##"xlink:href="#p-g""##,
        ] {
            assert!(out.contains(expected), "{expected} missing from {out}");
        }
    }

    #[test]
    fn a_body_without_ids_is_left_byte_identical() {
        let html = r##"<p>Plain <a href="/other/">link</a> and <a href="#top">top</a>.</p>"##;
        assert_eq!(scope_ids(html, "posts-x"), html);
    }

    #[test]
    fn a_scope_never_collides_with_one_handed_out_or_an_id_on_the_page() {
        let body = r#"<h2 id="intro">x</h2>"#;
        let mut scopes = IdScopes::around(r#"<h2 id="posts-a-intro">x</h2>"#);
        assert!(scopes.scope(body, "posts/b/index.html", 0).contains(r#"id="posts-b-intro""#));
        assert!(scopes.scope(body, "posts/b/index.html", 1).contains(r#"id="item-2-intro""#), "the same page drawn twice");
        assert!(scopes.scope(body, "posts/a/index.html", 2).contains(r#"id="item-3-intro""#), "the page already holds posts-a-intro");
        assert!(scopes.scope(body, "index.html", 3).contains(r#"id="item-4-intro""#));
    }

    #[test]
    fn two_bodies_whose_prefixed_ids_meet_are_told_apart_in_either_order() {
        let a = ("posts/a/index.html", r#"<h2 id="b-intro">x</h2>"#);
        let ab = ("posts/a-b/index.html", r#"<h2 id="intro">x</h2>"#);
        for [first, second] in [[a, ab], [ab, a]] {
            let mut scopes = IdScopes::default();
            let one = scopes.scope(first.1, first.0, 0);
            let two = scopes.scope(second.1, second.0, 1);
            assert!(one.contains(r#"id="posts-a-b-intro""#), "{one}");
            assert!(!two.contains(r#"id="posts-a-b-intro""#), "{two}");
        }
    }

    #[test]
    fn a_percent_encoded_fragment_follows_its_id() {
        let out = scope_ids(r##"<h2 id="intro">I</h2><a href="#%69ntro">i</a>"##, "p");
        assert!(out.contains(r##"href="#p-%69ntro""##), "{out}");
    }
}
