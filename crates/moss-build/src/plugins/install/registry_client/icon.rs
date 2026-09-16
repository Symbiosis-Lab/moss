//! The icon for a plugin moss does not ship.
//!
//! A bundled plugin's icon is in the binary and an installed one's is on disk;
//! a row for a plugin that is only *published* has neither, and its icon lives
//! at a URL. The frontend never fetches that URL — icons reach every surface
//! through one resolver, and a webview that fetched catalog icons would leak a
//! read of the settings pane to whatever host the index named.
//!
//! So the download happens here, at refresh time, and what lands on disk is
//! already safe to render. That matters more than it looks: the icon is the
//! one thing an index entry names that the `sha256` does **not** cover. The
//! artifact is pinned at review; the icon bytes can change afterwards without
//! anything noticing. And the glyph is written into the privileged webview
//! with `innerHTML`, where `<svg onload=…>` is script — reachable from a
//! window that can call every Tauri command.
//!
//! Two rules follow, both applied before the bytes are stored (sanitize at
//! ingest, ADR-025 §11 — the same posture as the comment sanitizer in
//! `moss-build`, for the same reason):
//!
//! 1. The icon must come from the same origin as the artifact whose hash was
//!    reviewed. It does not make the icon reviewed, but it stops an entry from
//!    pointing the app at a host nobody vetted.
//! 2. Only an allowlist of drawing elements and attributes survives.

use std::path::{Path, PathBuf};

use super::index::IndexEntry;

/// Shape elements and the container. No `<script>`, no `<foreignObject>` (it
/// re-enters HTML parsing), no `<image>` or `<use>` (both fetch), no `<a>`.
const ALLOWED_TAGS: &[&str] = &[
    "svg", "g", "path", "circle", "ellipse", "line", "polyline", "polygon", "rect", "title",
    "defs", "linearGradient", "radialGradient", "stop", "clipPath", "mask",
];

/// Geometry and paint only. Every `on*` handler is absent by construction —
/// this is an allowlist, so a handler moss has never heard of is dropped too,
/// which is the property a blocklist cannot have.
///
/// No `class`: these bytes land in the app's own document, where a class is a
/// claim on moss's stylesheet. `id` stays because a gradient has to be
/// referable, but namespaced — see [`NAMESPACE`].
const ALLOWED_ATTRS: &[&str] = &[
    "viewBox", "xmlns", "width", "height", "fill", "stroke", "stroke-width", "stroke-linecap",
    "stroke-linejoin", "stroke-dasharray", "stroke-opacity", "fill-opacity", "fill-rule",
    "clip-rule", "opacity", "d", "cx", "cy", "r", "rx", "ry", "x", "y", "x1", "y1", "x2", "y2",
    "points", "transform", "offset", "stop-color", "stop-opacity", "gradientUnits", "id",
    "clip-path", "mask",
];

/// The cached icon for a published plugin, if one was downloaded.
///
/// Keyed by id **and version**, so a plugin that publishes a new icon with a
/// new version cannot be served the old one: a different version is a
/// different file, and staleness stops being a state anything has to detect.
pub fn cached_icon(app_data_dir: &Path, entry: &IndexEntry) -> Option<String> {
    let svg = std::fs::read_to_string(icon_path(app_data_dir, &entry.id, &entry.version)).ok()?;
    // Empty is the refused-icon marker `cache_icons` leaves, not an icon.
    (!svg.is_empty()).then_some(svg)
}

/// Download and store the icon for every entry that names one.
///
/// Best-effort by design, and called from the refresh that already degrades to
/// the cache: an icon that will not download costs the row its glyph and
/// nothing else — [`crate::plugins::registry::resolve_channel_icon`] falls
/// through to the letter fallback the catalog already draws for a plugin with
/// no icon at all.
pub fn cache_icons(app_data_dir: &Path, entries: &[IndexEntry]) {
    for entry in entries {
        // An id moss ships never reaches `published_icon`: `resolve_channel_icon`
        // returns the bundled icon first. Fetching one is a request to whatever
        // host the index named, at launch, for a picture nothing will read —
        // and it tells that host the user is running moss before the user has
        // opened anything.
        if crate::plugins::bundled::get_bundled_plugin_names().contains(&entry.id.as_str()) {
            continue;
        }
        let Some(url) = entry.icon_url.as_deref() else {
            continue;
        };
        let path = icon_path(app_data_dir, &entry.id, &entry.version);
        if path.exists() {
            continue;
        }
        if !same_origin(url, &entry.download_url) {
            log::warn!(
                "registry icon for {} is not served from the artifact's origin, ignoring: {url}",
                entry.id
            );
            continue;
        }
        let raw = match super::fetch::get_document_capped(url, super::fetch::MAX_ICON_BYTES) {
            Ok(raw) => raw,
            Err(e) => {
                log::info!("registry icon for {} not fetched: {e}", entry.id);
                continue;
            }
        };
        let svg = match sanitize_icon_svg(&raw, &entry.id, &entry.version) {
            Some(svg) => svg,
            None => {
                log::warn!("registry icon for {} is not an SVG moss will render", entry.id);
                // An empty file is the answer "asked, nothing to show". Without
                // it this refetches the same rejected bytes on every refresh
                // for as long as the entry is published. Only the verdict that
                // cannot change is remembered — a fetch failure is transient
                // and falls through to `continue`, so a flaky network does not
                // cost the plugin its icon forever. A new version gets a new
                // path, so a publisher's fix is picked up.
                String::new()
            }
        };
        // `write_atomic` and not a hand-rolled write: two refreshes can
        // overlap — `REFRESHING` is cleared before icons run, deliberately, so
        // the kill switch does not wait on them — and a shared staging name
        // lets one writer's truncate meet the other's rename, landing a 0-byte
        // file that `cached_icon` then reads as the never-refetch marker. That
        // is the overlap the helper's own docstring names the registry for.
        if let Err(e) = crate::infra::atomic_write::write_atomic(&path, &svg) {
            log::warn!("registry icon for {} not stored: {e}", entry.id);
        }
    }
    sweep(app_data_dir, entries);
}

/// Delete every cached icon the current index does not name.
///
/// The cache is keyed by id AND version, so a plugin that publishes a new
/// icon leaves its old file behind, and nothing else in the process ever looks
/// at it again — the directory only grew. Here is the one place that knows the
/// whole set of names that should exist, which is why the sweep lives at the
/// end of the write rather than in a timer somewhere: the answer is already in
/// hand.
///
/// Only `.svg` files, deliberately. `write_atomic` puts its temp file beside
/// the destination, and two refreshes can overlap — deleting a name this sweep
/// does not recognize would be deleting another writer's file out from under
/// its own rename. A temp stranded by a kill is left behind; nothing reads
/// one, and the alternative costs an in-flight icon.
fn sweep(app_data_dir: &Path, entries: &[IndexEntry]) {
    let keep: std::collections::HashSet<PathBuf> =
        entries.iter().map(|entry| icon_path(app_data_dir, &entry.id, &entry.version)).collect();
    let Ok(cached) = std::fs::read_dir(app_data_dir.join("plugin-icons")) else {
        return;
    };
    for file in cached.flatten() {
        let path = file.path();
        let ours = path.extension().and_then(|e| e.to_str()) == Some("svg");
        if ours && path.is_file() && !keep.contains(&path) {
            if let Err(e) = std::fs::remove_file(&path) {
                log::warn!("stale registry icon {} not removed: {e}", path.display());
            }
        }
    }
}

fn icon_path(app_data_dir: &Path, id: &str, version: &str) -> PathBuf {
    // Both halves, not just the version. `id` is charset-validated on the way
    // out of the index (`is_usable_id`) and that is what actually holds here —
    // but this function builds a path, and a path builder that relies on a
    // check in another module is one refactor away from not holding. The
    // version has no such check at all: it is compared as a semver and
    // otherwise carried as whatever string the index said.
    app_data_dir.join("plugin-icons").join(format!("{}.svg", icon_key(id, version)))
}

/// The one name for an icon: a digest of its id AND version.
///
/// Not a sanitized spelling of the two strings. Joining them with a separator
/// needs that separator not to occur inside either half, and two rounds of
/// trying produced two collisions — `id="a-b"` with `1.0.0` against `id="a"`
/// with `b-1.0.0`, and then, once `-` was folded to `_`, `a-b` against `a_b`.
/// Neither half is validated where this is called from
/// [`crate::plugins::registry::get_installed_plugin_icon`]: the id is a
/// directory name off disk and the version is whatever a `manifest.json` says.
/// A digest has no separator to hide, so the ambiguity is gone rather than
/// bounded, and the charset argument goes with it.
fn icon_key(id: &str, version: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    // Length-prefixed, so `("ab", "c")` and `("a", "bc")` do not hash alike
    // either — the same collision one layer down.
    hasher.update(id.len().to_le_bytes());
    hasher.update(id.as_bytes());
    hasher.update(version.as_bytes());
    hex::encode(&hasher.finalize()[..8])
}

/// Whether two URLs share a scheme, host and port.
///
/// Not a string prefix test: `https://example.org.evil.test/x` starts with
/// neither more nor less of `https://example.org/` than a parser would notice,
/// and prefix tests on URLs are how that gets missed.
fn same_origin(icon_url: &str, artifact_url: &str) -> bool {
    let (Ok(icon), Ok(artifact)) = (url::Url::parse(icon_url), url::Url::parse(artifact_url))
    else {
        return false;
    };
    icon.scheme() == artifact.scheme()
        && icon.host_str() == artifact.host_str()
        && icon.port_or_known_default() == artifact.port_or_known_default()
}

/// The attributes on [`ALLOWED_ATTRS`] whose value a renderer will go and
/// fetch. The list is closed because the attribute allowlist is: these five
/// are every remaining one that takes a reference.
const REFERENCE_ATTRS: &[&str] = &["fill", "stroke", "stop-color", "clip-path", "mask"];

/// Every `id` an icon declares, and every reference to one, is rewritten to
/// start with this — id and version included, so it is unique to ONE icon.
///
/// `id` is document-global, and the catalog draws every tile into the same
/// document. A single shared prefix would only separate icons from moss's own
/// ids: two publishers both shipping `id="g"` would still collide, and
/// whichever tile rendered first would decide what the other one painted with.
/// It is also a name-claiming defence — an icon must not be able to take an id
/// the settings pane looks up, or one the browser exposes as a `window`
/// property (DOM clobbering), inside the webview that can call every Tauri
/// command.
fn namespace_for(id: &str, version: &str) -> String {
    format!("moss-icon-{}-", icon_key(id, version))
}

/// Rewrite a paint or reference value so it stays inside THIS icon, or refuse
/// it.
///
/// An ALLOWLIST, and deliberately so. The first version of this refused
/// anything spelled `url(` with a non-local target, which is a blocklist
/// wearing a different hat: it missed `mask="image-set('https://…')"` — SVG2
/// maps `mask` onto the CSS shorthand, which takes an `<image>`, so no `url(`
/// need appear — and CSS ident escapes, where `\75 rl(…)` is `url(…)` by the
/// time the tokenizer is done. Enumerating spellings of "fetch" is the losing
/// side of that game, the same way enumerating spellings of `manifest.json`
/// was on the install path. So: say what may pass, and let everything else go.
///
/// Three things may pass — a reference to something defined in this same icon,
/// a colour keyword or hex literal, and a colour function. Nothing else, which
/// includes every relative URL: `url(evil.svg#f)` carries no `:` or `/` and is
/// refused for not being local, not for looking remote.
///
/// Deciding and rewriting are ONE pass on purpose. Testing a lowercased copy
/// and then rewriting the original by string replacement let `URL(#g)` pass
/// the test and skip the rewrite — a reference that then resolved against
/// another icon's namespaced definition, which is the collision the namespace
/// exists to stop.
fn local_reference_value(value: &str, namespace: &str) -> Option<String> {
    // Token by token: `mask` is an SVG2 shorthand and takes a reference plus a
    // mode (`url(#m) luminance`), so a whole-value test refuses a legal icon.
    // Splitting cannot loosen the rule — every token still has to pass on its
    // own, and a fetch cannot be assembled out of two tokens that individually
    // carry no scheme, path or call.
    let mut out: Vec<String> = Vec::new();
    for token in split_outside_parens(value)? {
        out.push(local_reference_token(&token, namespace)?);
    }
    Some(out.join(" "))
}

/// Split on whitespace that is not inside a function call.
///
/// `rgb(0 0 0 / 50%)` is ONE token. Splitting it on plain whitespace made
/// three of its arguments look like bare values and the `/` like a path, so a
/// legal modern colour — what most design tools emit today — lost its
/// attribute and painted opaque black. Unbalanced parentheses are refused
/// outright: there is no reading of them that is worth guessing at.
fn split_outside_parens(value: &str) -> Option<Vec<String>> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut depth = 0usize;
    for ch in value.chars() {
        match ch {
            '(' => {
                depth += 1;
                current.push(ch);
            }
            ')' => {
                depth = depth.checked_sub(1)?;
                current.push(ch);
            }
            c if c.is_whitespace() && depth == 0 => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            }
            c => current.push(c),
        }
    }
    if depth != 0 {
        return None;
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    Some(tokens)
}

fn local_reference_token(token: &str, namespace: &str) -> Option<String> {
    let lower = token.to_ascii_lowercase();
    if lower.starts_with("url(") {
        // Case is compared folded and the target is carried through as
        // written: an id is case-sensitive, so lowercasing the reference here
        // would leave it pointing at nothing.
        let inner = token.get(4..)?.strip_suffix(')')?;
        let target = inner.trim().trim_matches(['"', '\'']);
        let name = target.strip_prefix('#')?;
        if name.is_empty() || name.contains([':', '/']) {
            return None;
        }
        // Emitted in one canonical spelling, so what lands in the document is
        // what this function decided rather than what the publisher typed.
        return Some(format!("url(#{namespace}{name})"));
    }
    if let Some((function, arguments)) = token.split_once('(') {
        // A colour function is the only call that is not a fetch. Anything
        // else taking arguments — `image-set`, `image`, `element`, `-webkit-*`
        // — is refused without moss needing to have heard of it.
        if !matches!(
            function.to_ascii_lowercase().as_str(),
            "rgb"
                | "rgba"
                | "hsl"
                | "hsla"
                | "hwb"
                | "lab"
                | "lch"
                | "oklab"
                | "oklch"
                | "color"
                | "color-mix"
        ) {
            return None;
        }
        // And its arguments are numbers and separators, nothing else. `/` is
        // among them because that is how CSS Color 4 spells alpha —
        // `rgb(0 0 0 / 50%)` — which is why this cannot be judged by the
        // two-character test that used to close this function.
        let arguments = arguments.strip_suffix(')')?;
        // Letters belong in there — `hsl(210deg …)`, `color-mix(in srgb, …)`,
        // `oklch(… / none)`. Digits alone was the same mistake one size
        // smaller: it closed the one spelling the test was written for and
        // repainted `hsl(210deg 50% 40%)` and `oklch(…)` black, which is the
        // failure this grammar exists to stop. No parenthesis is in the set,
        // so no call can nest inside a colour and `var(--x, url(…))` never
        // arrives — `var` is not on the list above either way.
        let colour_argument = |c: char| {
            c.is_ascii_alphanumeric() || matches!(c, '.' | '%' | ',' | '/' | '+' | '-' | ' ' | '\t')
        };
        return arguments.chars().all(colour_argument).then(|| token.to_string());
    }
    // Everything left is a bare value, and it says what it may BE rather than
    // what it may not contain. The previous closing line — no `:` and no `/` —
    // was a two-character blocklist inside the function whose whole docstring
    // says not to write one: CSS ident escapes spell both, so
    // `rgb(0,0,0)image-set(https\3a\2f\2fh.test\2fx.svg)` passed it whole.
    if let Some(hex) = token.strip_prefix('#') {
        return (matches!(hex.len(), 3 | 4 | 6 | 8) && hex.chars().all(|c| c.is_ascii_hexdigit()))
            .then(|| token.to_string());
    }
    // A keyword — `none`, `currentColor`, `red`, `luminance`, `evenodd`.
    if !lower.is_empty() && lower.chars().all(|c| c.is_ascii_alphabetic()) {
        return Some(token.to_string());
    }
    // Or a number, with or without a unit sign: stroke widths and offsets.
    let number = token.strip_suffix('%').unwrap_or(token);
    let number = number.strip_prefix(['+', '-']).unwrap_or(number);
    (!number.is_empty() && number.chars().all(|c| c.is_ascii_digit() || c == '.'))
        .then(|| token.to_string())
}

/// Reduce downloaded markup to an SVG that is safe to write into the app's own
/// DOM, or refuse it.
///
/// `id` and `version` name the icon, and only so its ids can be namespaced to
/// it — see [`namespace_for`].
///
/// `None` when nothing that draws survives, so a caller cannot cache a blank
/// file and call it an icon — see [`draws_something`], which is the half of
/// that promise the first version did not keep.
pub(crate) fn sanitize_icon_svg(raw: &str, id: &str, version: &str) -> Option<String> {
    let namespace = namespace_for(id, version);
    let cleaned = ammonia::Builder::default()
        .tags(ALLOWED_TAGS.iter().copied().collect())
        .generic_attributes(ALLOWED_ATTRS.iter().copied().collect())
        // Belt to the allowlist's braces: no attribute ammonia treats as a URL
        // is in `ALLOWED_ATTRS`, so this changes nothing today. It is here so
        // that adding one later cannot quietly admit a scheme.
        .url_schemes(std::collections::HashSet::new())
        // …and `url_schemes` is not enough, because it governs only ammonia's
        // own url-bearing attributes (`a href`, `img src`). The five in
        // REFERENCE_ATTRS are not in that set, so
        // `mask="url(https://tracker.invalid/m.svg#m)"` sailed through the
        // allowlist and WebKit fetched it on render — a settings pane that
        // beacons out for a plugin the user never installed, through the one
        // field `sha256` does not cover. Same reasoning that keeps `<image>`
        // and `<use>` out of ALLOWED_TAGS, one layer further in.
        .attribute_filter(move |_element, attribute, value| {
            if attribute == "id" {
                return Some(format!("{namespace}{value}").into());
            }
            if !REFERENCE_ATTRS.contains(&attribute) {
                return Some(value.into());
            }
            local_reference_value(value, &namespace).map(Into::into)
        })
        .clean(raw)
        .to_string();
    (cleaned.contains("<svg") && draws_something(&cleaned)).then_some(cleaned)
}
/// Whether the cleaned markup contains anything that paints.
///
/// A SECOND parse, not a scan of the first one's output. The allowlist has no
/// `text`, `use`, `image` or `symbol`, so a letterform logo —
/// `<svg><text>M</text></svg>` — survives as an `<svg>` wrapping a bare text
/// node: valid markup, zero drawing, and `isValidSvg` on the frontend says
/// yes. Cached, that is a permanently blank tile which never falls back to the
/// letter glyph and never refetches, because the file exists. Refusing here is
/// what makes the fallback happen.
///
/// Three versions of this answered by scanning the string for `<path` and
/// friends, and each had a door the next one closed: `<line` prefixes
/// `<linearGradient`; a tag name that does not end where a tag name ends;
/// `<defs>` inside `<defs>`. All three are the same mistake — re-deriving a
/// parse from a serialization — so the fourth report against this function was
/// answered by removing the scanner rather than the door.
/// Ammonia is the only producer of this string and it is already a dependency:
/// asking it again — drawing elements and their containers allowed, definition
/// containers stripped whole, and every attribute dropped — answers on a parse
/// tree instead. With no attributes left, the serialization is canonical, so
/// `<path>` is an element and can be nothing else.
fn draws_something(cleaned: &str) -> bool {
    // `svg` and `g` are here only to carry the geometry: ammonia drops an
    // entire foreign subtree whose root it does not allow, so without them
    // this pass returns nothing for every icon ever written.
    let mut tags: std::collections::HashSet<&str> =
        GEOMETRY.iter().map(|(element, _, _)| *element).collect();
    tags.insert("svg");
    tags.insert("g");
    // Everything else this sanitizer allows is a DEFINITION, and geometry
    // inside one is not ink — otherwise a letterform logo passes on the
    // strength of the gradient it declares. DERIVED, not listed: a hand-kept
    // list of containers is what let `<linearGradient>` outside `<defs>`
    // through, which is the same bug this function has now been reported for
    // five rounds running. Adding a tag to `ALLOWED_TAGS` classifies it here
    // in the same edit.
    let definitions: std::collections::HashSet<&str> =
        ALLOWED_TAGS.iter().copied().filter(|tag| !tags.contains(tag)).collect();
    // The verdict is collected from the FILTER, not read back out of the
    // serialized result. Ammonia calls it once per surviving attribute of a
    // surviving element and never inside a stripped container, so what arrives
    // here is already the parse tree's own answer — no string of markup to
    // re-read, and no attribute value that could be mistaken for one.
    let painted: std::sync::Arc<std::sync::Mutex<std::collections::HashSet<String>>> =
        std::sync::Arc::new(std::sync::Mutex::new(std::collections::HashSet::new()));
    let sink = painted.clone();
    let _ = ammonia::Builder::default()
        .tags(tags)
        .clean_content_tags(definitions)
        .generic_attributes(GEOMETRY.iter().map(|(_, attribute, _)| *attribute).collect())
        .attribute_filter(move |element, attribute, value| {
            if paints(element, attribute, value) {
                if let Ok(mut seen) = sink.lock() {
                    seen.insert(format!("{element}:{attribute}"));
                }
            }
            Some(value.into())
        })
        .clean(cleaned);

    let Ok(painted) = painted.lock() else { return false };
    // A rect needs both of its sides; everything else needs one of its own
    // attributes. The residual: two half-specified rects look like one whole
    // one, because ammonia names the element but does not identify it. That
    // costs a blank tile in a case nobody writes by accident, where the
    // alternative is a second parser.
    if painted.contains("rect:width") && painted.contains("rect:height") {
        return true;
    }
    GEOMETRY
        .iter()
        .filter(|(element, _, _)| *element != "rect")
        .any(|(element, attribute, _)| painted.contains(&format!("{element}:{attribute}")))
}

/// The attribute that makes each drawing element visible, and whether an
/// absent-or-zero value means it is not there.
///
/// `<path>` with no `d`, `<circle>` with no `r`, `<rect width="0">` — all of
/// them survive every allowlist in this file and paint nothing. Element names
/// alone were the check, so an icon made of them cached as a permanently blank
/// tile which never refetches, which is the one outcome
/// [`draws_something`] exists to prevent.
const GEOMETRY: &[(&str, &str, bool)] = &[
    ("path", "d", false),
    ("polyline", "points", false),
    ("polygon", "points", false),
    ("circle", "r", true),
    ("ellipse", "rx", true),
    ("ellipse", "ry", true),
    ("rect", "width", true),
    ("rect", "height", true),
    ("line", "x1", true),
    ("line", "y1", true),
    ("line", "x2", true),
    ("line", "y2", true),
];

fn paints(element: &str, attribute: &str, value: &str) -> bool {
    GEOMETRY.iter().any(|(tag, name, numeric)| {
        *tag == element
            && *name == attribute
            && if *numeric {
                value.trim().parse::<f64>().is_ok_and(|n| n != 0.0)
            } else {
                !value.trim().is_empty()
            }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str, version: &str) -> IndexEntry {
        serde_json::from_value(serde_json::json!({
            "type": "plugin",
            "id": id,
            "display_name": id,
            "version": version,
            "download_url": "https://example.invalid/x.zip",
            "sha256": "0".repeat(64),
        }))
        .unwrap()
    }

    /// The marker `cache_icons` writes when the bytes it fetched were not an
    /// icon it will render. Read back as an icon it would be an empty string,
    /// and the tile would draw nothing instead of falling back to the letter
    /// glyph — a refusal that looks exactly like a broken renderer.
    #[test]
    fn a_remembered_refusal_is_not_an_icon() {
        let tmp = tempfile::tempdir().unwrap();
        let entry = entry("stranger", "1.0.0");
        let path = icon_path(tmp.path(), &entry.id, &entry.version);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();

        std::fs::write(&path, "").unwrap();
        assert_eq!(cached_icon(tmp.path(), &entry), None, "the marker is not a drawing");

        std::fs::write(&path, "<svg></svg>").unwrap();
        assert_eq!(cached_icon(tmp.path(), &entry).as_deref(), Some("<svg></svg>"));
    }

    /// The attack this module exists to stop, in the form it actually takes.
    ///
    /// `innerHTML` does not run a `<script>` tag, which is what makes people
    /// think markup injection is survivable. It does run `onload` on an SVG
    /// element, and the window it runs in can call every Tauri command.
    #[test]
    fn a_handler_hidden_in_a_downloaded_icon_does_not_reach_the_dom() {
        let hostile = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"
            onload="alert(1)"><path d="M1 1h2" onclick="steal()"/>
            <script>alert(2)</script>
            <foreignObject><img src=x onerror="alert(3)"></foreignObject>
            <image href="https://tracker.invalid/px.gif"/>
            <a href="javascript:alert(4)">x</a></svg>"#;

        let cleaned = sanitize_icon_svg(hostile, "x", "1.0.0").expect("the drawing survives");

        for forbidden in ["onload", "onclick", "onerror", "<script", "foreignObject", "<image", "javascript:"] {
            assert!(
                !cleaned.contains(forbidden),
                "{forbidden} survived sanitization: {cleaned}"
            );
        }
        assert!(cleaned.contains("<path"), "and the actual glyph is still there: {cleaned}");
    }

    /// The leak that is not script, and is not in `icon_url`.
    ///
    /// `fill`/`stroke`/`mask`/`clip-path` take a `url(…)` value. Ammonia's
    /// scheme filter does not see them — it governs `a href` and `img src` —
    /// so without the attribute filter these survive verbatim and the
    /// renderer fetches them: measured, WebKit fetches the `mask` and Chromium
    /// fetches all of them. That is the settings pane telling a third-party
    /// host the user's IP, for a plugin they never installed.
    #[test]
    fn a_paint_value_cannot_send_the_renderer_to_another_host() {
        let ns = namespace_for("x", "1.0.0");
        let beacon = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24">
            <path d="M1 1h2" fill="url(https://tracker.invalid/f.svg#f)"
                  stroke="url('https://tracker.invalid/s.svg#s')"
                  mask="url(https://tracker.invalid/m.svg#m)"
                  clip-path="url(//tracker.invalid/c.svg#c)"/>
            <rect x="0" y="0" width="4" height="4" fill="url(#local)"/></svg>"#;

        let cleaned = sanitize_icon_svg(beacon, "x", "1.0.0").expect("the drawing still survives");

        assert!(
            !cleaned.contains("tracker.invalid"),
            "no attribute may point off this document: {cleaned}"
        );
        assert!(cleaned.contains("<path"), "and the glyph is kept, not the whole element dropped");
        assert!(
            cleaned.contains(&format!("url(#{ns}local)")),
            "a reference to a gradient inside the same icon is not a fetch: {cleaned}"
        );
    }

    /// An icon's ids are its own, and its classes are not moss's to take.
    ///
    /// `id` is document-global: two tiles publishing `id="g"` share one
    /// gradient, and an id the settings pane looks up — or that the browser
    /// exposes as a `window` property — is a name a published icon must not be
    /// able to claim inside the privileged webview. `class` is worse in kind:
    /// it is a claim on moss's own stylesheet.
    #[test]
    fn a_published_icon_cannot_claim_a_name_in_the_document_it_lands_in() {
        let ns = namespace_for("x", "1.0.0");
        let squatter = r##"<svg viewBox="0 0 24 24" id="preview-frame" class="moss-overlay">
            <defs><linearGradient id="g"><stop offset="0" stop-color="#fff"/></linearGradient></defs>
            <path d="M1 1h2" fill="url(#g)"/></svg>"##;

        let cleaned = sanitize_icon_svg(squatter, "x", "1.0.0").expect("the drawing survives");

        assert!(!cleaned.contains("class="), "no class reaches moss's stylesheet: {cleaned}");
        assert!(!cleaned.contains(r#"id="preview-frame""#), "{cleaned}");
        assert!(cleaned.contains(&format!(r#"id="{ns}preview-frame""#)), "{cleaned}");
        // And the icon still paints: definition and reference moved together.
        assert!(cleaned.contains(&format!(r#"id="{ns}g""#)), "{cleaned}");
        assert!(cleaned.contains(&format!("url(#{ns}g)")), "{cleaned}");
    }

    /// Two icons in one document, which is what the catalog actually renders.
    ///
    /// A single shared prefix passes the one-icon test above and still lets
    /// two publishers collide on `id="g"`: whichever tile rendered first would
    /// own the name, and the second would paint with its neighbour's gradient
    /// — or, with a mask, render invisible. The namespace has to be per icon.
    #[test]
    fn two_icons_that_chose_the_same_id_do_not_share_it() {
        let icon = r##"<svg viewBox="0 0 1 1">
            <defs><linearGradient id="g"><stop offset="0" stop-color="#fff"/></linearGradient></defs>
            <path d="M0 0h1" fill="url(#g)"/></svg>"##;

        let a = sanitize_icon_svg(icon, "alpha", "1.0.0").unwrap();
        let b = sanitize_icon_svg(icon, "beta", "1.0.0").unwrap();
        // Compared against EACH OTHER, not against what `namespace_for`
        // returns. Asserting each output contains its own `namespace_for(…)`
        // is true of a single shared prefix as well, so that pair of
        // assertions could not fail against the collision this test is named
        // for — which is what the earlier form did.
        assert_ne!(a, b, "two publishers both shipping id=\"g\" must not share it");
        // The reference is rewritten to match the definition, whatever the
        // name turns out to be — the pair has to move together or the icon
        // paints with nothing.
        let named = a
            .split(r#"id=""#)
            .nth(1)
            .and_then(|rest| rest.split('"').next())
            .expect("the gradient kept a name")
            .to_string();
        assert!(named.starts_with("moss-icon-"), "{a}");
        assert!(named.ends_with("g"), "the publisher's own name survives inside it: {a}");
        assert!(a.contains(&format!(r#"fill="url(#{named})""#)), "{a}");

        // And a new version of one icon is not the old one either — the cache
        // is keyed by version, so both can be on screen across a refresh.
        let a2 = sanitize_icon_svg(icon, "alpha", "2.0.0").unwrap();
        assert_ne!(a, a2, "id and version both name the icon");

        // The names are a digest precisely so that no separator has to be
        // absent from either half. Each of these pairs named ONE icon under
        // some earlier spelling of the name: the first two under the original
        // `-`-joined form, the second also under the `_`-folded form that
        // replaced it, and the third under any join without a length prefix.
        assert_ne!(namespace_for("a-b", "1.0.0"), namespace_for("a", "b-1.0.0"));
        assert_ne!(namespace_for("a-b", "1.0.0"), namespace_for("a_b", "1.0.0"));
        assert_ne!(namespace_for("ab", "c"), namespace_for("a", "bc"));
    }

    /// The shapes that look like geometry and are not.
    ///
    /// Nesting is the one that shipped broken: the outer block's tail stayed
    /// visible to the scanner this replaced, so a `<rect>` parked there read as
    /// ink and the tile cached blank forever. The attribute leg holds a rule
    /// rather than a past bug — ammonia escapes a `<` inside an attribute
    /// value, so no scanner saw it either; it is here because "an attribute
    /// value is not an element" is exactly what parsing guarantees and
    /// scanning only happened to.
    #[test]
 
    /// The colour syntax every design tool emits today.
    ///
    /// `rgb(0 0 0 / 50%)` is one value with spaces in it. Split on plain
    /// whitespace, its `/` looked like a path and the whole `fill` was
    /// dropped — a legal icon silently repainted opaque black. The same pass
    /// must still refuse a fetch assembled out of ident escapes, which is what
    /// the two-character test it replaced could not see.
    #[test]
    fn a_modern_colour_keeps_its_paint_and_an_escaped_fetch_does_not() {
        let modern = r##"<svg viewBox="0 0 1 1"><path d="M0 0h1" fill="rgb(0 0 0 / 50%)"
            stroke="#abc" stroke-width="1.5" stroke-opacity="50%"/>
            <circle cx="1" cy="1" r="1" fill="hsl(210deg 50% 40%)" stroke="oklch(0.7 0.1 200)"/>
            <rect x="0" y="0" width="1" height="1" stroke-dasharray="4 2" fill="none"/></svg>"##;
        let cleaned = sanitize_icon_svg(modern, "x", "1.0.0").expect("draws");
        assert!(cleaned.contains("rgb(0 0 0 / 50%)"), "{cleaned}");
        // A unit and a colour space are letters, and a grammar of digits
        // refused both — the same repaint, one spelling along.
        assert!(cleaned.contains("hsl(210deg 50% 40%)"), "{cleaned}");
        assert!(cleaned.contains("oklch(0.7 0.1 200)"), "{cleaned}");
        assert!(cleaned.contains(r##"stroke="#abc""##), "{cleaned}");

        let escaped = r##"<svg viewBox="0 0 1 1"><path d="M0 0h1"
            fill="rgb(0,0,0)image-set(https\3a\2f\2fh.test\2fx.svg)"/></svg>"##;
        let cleaned = sanitize_icon_svg(escaped, "x", "1.0.0").expect("draws");
        assert!(!cleaned.contains("h.test"), "{cleaned}");
    }

    /// A reference spelled in a case the rewrite did not expect.
    ///
    /// The first version tested a lowercased copy and rewrote the original by
    /// string replacement, so `URL(#g)` passed the test and skipped the
    /// rewrite: it reached the document unchanged, pointing at whatever
    /// `#g` another icon had already defined. Deciding and rewriting are one
    /// pass now, and the output has one spelling.
    #[test]
    fn a_reference_is_namespaced_however_it_was_spelled() {
        let ns = namespace_for("x", "1.0.0");
        let icon = r##"<svg viewBox="0 0 1 1">
            <defs><linearGradient id="G"><stop offset="0" stop-color="#fff"/></linearGradient></defs>
            <path d="M0 0h1" fill="URL(#G)" stroke="url('#G')"/></svg>"##;

        let cleaned = sanitize_icon_svg(icon, "x", "1.0.0").unwrap();
        assert_eq!(cleaned.matches(&format!("url(#{ns}G)")).count(), 2, "{cleaned}");
        assert!(!cleaned.contains("URL("), "no unrewritten spelling survives: {cleaned}");
        // Case is carried through: an id is case-sensitive, so a lowercased
        // reference would point at nothing.
        assert!(cleaned.contains(&format!(r#"id="{ns}G""#)), "{cleaned}");
    }

    /// Markup that sanitizes to something with no ink in it.
    ///
    /// Not a hostile case — a letterform logo is `<text>`, which is not in the
    /// allowlist because it is not geometry. What survives is an `<svg>`
    /// wrapping a bare text node: `isValidSvg` on the frontend says yes, the
    /// tile draws nothing, the file exists so nothing refetches, and the
    /// letter fallback never runs. The blank tile is permanent.
    #[test]
 
    /// Nothing that draws nothing becomes an icon.
    ///
    /// One test, because there is now one rule and it is decided in one place.
    /// This is the promise the whole file exists to keep: markup that survives
    /// the allowlist and then paints nothing is cached as an icon, the file
    /// exists so nothing refetches it, and the tile is blank forever instead of
    /// falling back to the letter glyph. Every refusal below is a shape that
    /// once got past a version of the check — the list is the bug history.
    #[test]
    fn nothing_without_ink_becomes_an_icon() {
        let refused = [
            // Not markup, or markup with no drawing element at all.
            ("<script>alert(1)</script>", "not an svg"),
            ("not markup at all", "not markup"),
            (r#"<svg viewBox="0 0 24 24"><text x="4" y="18">M</text></svg>"#, "a letterform logo"),
            (r#"<svg viewBox="0 0 1 1"><noscript>x</noscript></svg>"#, "nothing that draws"),
            // A definition is not ink, wherever it sits. The gradient carries
            // real geometry and needs no `<defs>` wrapper: that pair is what
            // beat a hand-written list of containers.
            (
                r##"<svg viewBox="0 0 1 1"><linearGradient id="g"><path d="M0 0h1"/>
                    </linearGradient><text>M</text></svg>"##,
                "a gradient outside defs",
            ),
            (
                r#"<svg viewBox="0 0 1 1"><clipPath id="c"><path d="M0 0h1"/></clipPath></svg>"#,
                "a clip path defines a shape, it does not draw one",
            ),
            (
                r##"<svg viewBox="0 0 1 1"><defs><defs></defs>
                    <rect x="0" y="0" width="1" height="1"/></defs></svg>"##,
                "geometry in a nested definition",
            ),
            // An attribute value is not an element. Ammonia escapes the `<`,
            // so no scanner saw this either — it is here because parsing
            // guarantees what scanning only happened to.
            (
                r##"<svg viewBox="0 0 1 1" transform="<path >"><text>M</text></svg>"##,
                "markup inside an attribute",
            ),
            // And an element is not ink: these are what a drawing element is
            // called with nothing to draw.
            (r##"<svg viewBox="0 0 1 1"><path/></svg>"##, "a path with no d"),
            (r##"<svg viewBox="0 0 1 1"><path d=""/></svg>"##, "a path with an empty d"),
            (r##"<svg viewBox="0 0 1 1"><circle cx="1" cy="1"/></svg>"##, "a circle with no r"),
            (
                r##"<svg viewBox="0 0 1 1"><rect x="0" y="0" width="0" height="4"/></svg>"##,
                "a rect with no width",
            ),
            (r##"<svg viewBox="0 0 1 1"><rect x="0" y="0" width="4"/></svg>"##, "a rect with no height"),
            (
                r##"<svg viewBox="0 0 1 1"><line x1="0" y1="0" x2="0" y2="0"/></svg>"##,
                "a line of no length",
            ),
        ];
        for (markup, what) in refused {
            assert_eq!(sanitize_icon_svg(markup, "x", "1.0.0"), None, "{what}: {markup}");
        }

        // …and none of that costs a real icon its glyph.
        let drawn = [
            r##"<svg viewBox="0 0 1 1"><defs><linearGradient id="g">
                <stop offset="0" stop-color="#fff"/></linearGradient></defs>
                <path d="M0 0h1" fill="url(#g)"/></svg>"##,
            r#"<svg viewBox="0 0 1 1"><g><path d="M0 0h1"/></g></svg>"#,
            r##"<svg viewBox="0 0 1 1"><defs><defs></defs></defs>
                <rect x="0" y="0" width="1" height="1"/></svg>"##,
        ];
        for markup in drawn {
            assert!(sanitize_icon_svg(markup, "x", "1.0.0").is_some(), "a real icon: {markup}");
        }
    }

    /// The spellings that beat a `url(`-shaped rule.
    ///
    /// Each of these fetches without the four letters the first version of
    /// this looked for: `mask` takes the CSS `<image>` shorthand in SVG2, and
    /// a CSS ident escape spells `url` without spelling it. This is why the
    /// rule says what may pass instead of what may not.
    #[test]
    fn a_fetch_that_is_not_spelled_url_is_refused_too() {
        for hostile in [
            r#"<svg viewBox="0 0 1 1"><path d="M0 0" mask="image-set('https://tracker.invalid/m.svg' 1x)"/></svg>"#,
            r#"<svg viewBox="0 0 1 1"><path d="M0 0" fill="\75 rl(https://tracker.invalid/f.svg#f)"/></svg>"#,
            r#"<svg viewBox="0 0 1 1"><path d="M0 0" fill="URL(HTTPS://TRACKER.INVALID/F.SVG#F)"/></svg>"#,
            r#"<svg viewBox="0 0 1 1"><path d="M0 0" fill="url( 'https://tracker.invalid/f.svg#f' )"/></svg>"#,
            // A relative URL carries neither a scheme nor a slash, so a rule
            // that refused things for "looking remote" would pass it.
            r#"<svg viewBox="0 0 1 1"><path d="M0 0" fill="url(evil.svg#f)"/></svg>"#,
        ] {
            let cleaned = sanitize_icon_svg(hostile, "x", "1.0.0").expect("the drawing survives");
            assert!(
                !cleaned.to_ascii_lowercase().contains("tracker.invalid")
                    && !cleaned.contains("evil.svg"),
                "a fetch got through: {cleaned}"
            );
        }
    }

    /// And the ordinary paint values a real icon uses are not casualties.
    #[test]
    fn the_paint_a_real_icon_uses_still_passes() {
        let ns = namespace_for("x", "1.0.0");
        let real = r##"<svg viewBox="0 0 24 24"><path d="M1 1h2" fill="none" stroke="currentColor"/>
            <rect x="0" y="0" width="2" height="2" fill="#3b82f6"/>
            <circle cx="1" cy="1" r="1" fill="rgb(59, 130, 246)"/>
            <ellipse cx="1" cy="1" rx="1" ry="1" fill="url(#grad)" clip-path="url('#clip')"/></svg>"##;

        let cleaned = sanitize_icon_svg(real, "x", "1.0.0").expect("a real icon is not refused");

        for kept in [
            "fill=\"none\"".to_string(),
            "currentColor".to_string(),
            "#3b82f6".to_string(),
            "rgb(".to_string(),
            // Namespaced, and the definition it points at is namespaced with
            // it. A quoted reference comes out unquoted: the value is rebuilt
            // in one canonical spelling rather than patched, so what lands in
            // the document is what the filter decided.
            format!("url(#{ns}grad)"),
            format!("url(#{ns}clip)"),
        ] {
            assert!(cleaned.contains(&kept), "{kept} was refused: {cleaned}");
        }

        // The SVG2 shorthand: a reference plus a mode, which a whole-value
        // test refuses even though every token is local.
        let shorthand = r#"<svg viewBox="0 0 1 1"><path d="M0 0" mask="url(#m) luminance"/></svg>"#;
        let cleaned = sanitize_icon_svg(shorthand, "x", "1.0.0").expect("still an icon");
        assert!(cleaned.contains("luminance"), "a legal mask value was dropped: {cleaned}");
    }

    /// A sanitizer that eats `viewBox` ships square icons, which is how this
    /// gets reverted. HTML parsers lowercase attribute names; the SVG ones
    /// that matter are camelCase, so this is the failure to check for rather
    /// than assume.
    #[test]
    fn an_ordinary_icon_survives_intact_enough_to_draw() {
        let real = r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><rect width="20" height="16" x="2" y="4" rx="2"/><path d="m22 7-8.97 5.7a1.94 1.94 0 0 1-2.06 0L2 7"/></svg>"#;

        let cleaned = sanitize_icon_svg(real, "x", "1.0.0").expect("a real icon is not refused");

        assert!(cleaned.contains("viewBox=\"0 0 24 24\""), "viewBox must survive: {cleaned}");
        assert!(cleaned.contains("stroke-linecap"), "hyphenated paint attrs too: {cleaned}");
        assert!(cleaned.contains("<rect"), "and every shape: {cleaned}");
        assert!(cleaned.contains("<path"));
    }

    #[test]
 
    /// The icon is the one field the artifact hash does not cover, so the
    /// least moss can require is that it comes from where the artifact does.
    #[test]
    fn an_icon_hosted_somewhere_the_artifact_is_not_is_refused() {
        let artifact = "https://github.com/Symbiosis-Lab/moss-registry/releases/download/x-v1/x.zip";
        assert!(same_origin(
            "https://github.com/Symbiosis-Lab/moss-registry/releases/download/x-v1/icon.svg",
            artifact
        ));
        assert!(!same_origin("https://tracker.invalid/px.svg", artifact));
        // A prefix test passes this one. A parsed origin does not.
        assert!(!same_origin("https://github.com.evil.test/icon.svg", artifact));
        assert!(!same_origin("http://github.com/icon.svg", artifact), "scheme counts");
        assert!(!same_origin("not a url", artifact));
    }

    /// Version in the file name, so a republished icon cannot be served stale.
    #[test]
    fn a_new_version_reads_from_a_different_file() {
        let root = Path::new("/tmp/moss-icons");
        assert_ne!(
            icon_path(root, "stranger", "1.0.0"),
            icon_path(root, "stranger", "1.1.0")
        );
        // The property is not "no dots" — a version is full of dots — but
        // that the name stays one component inside the icon directory.
        let escaped = icon_path(root, "stranger", "../../etc/passwd");
        assert_eq!(
            escaped.parent(),
            Some(root.join("plugin-icons").as_path()),
            "a version is a name, not a path: {}",
            escaped.display()
        );
    }

    /// Which is why something has to delete the old one.
    ///
    /// Keying by version means a publisher's second icon never overwrites
    /// their first: without a sweep the directory only grows, for the whole
    /// life of the install.
    #[test]
    fn an_icon_the_index_no_longer_names_is_deleted() {
        let tmp = tempfile::tempdir().unwrap();
        let icons = tmp.path().join("plugin-icons");
        std::fs::create_dir_all(&icons).unwrap();

        let current = icon_path(tmp.path(), "stranger", "2.0.0");
        let superseded = icon_path(tmp.path(), "stranger", "1.0.0");
        // A concurrent refresh's temp file, mid-write. `write_atomic` names it
        // beside the destination, and it is not this sweep's to delete.
        let in_flight = icons.join(".other.svg.4242.7.tmp");
        for file in [&current, &superseded, &in_flight] {
            std::fs::write(file, "<svg/>").unwrap();
        }

        sweep(tmp.path(), &[entry("stranger", "2.0.0")]);

        assert!(current.exists(), "the version the index names stays");
        assert!(!superseded.exists(), "the version it replaced does not");
        assert!(in_flight.exists(), "another writer's temp file is not ours to remove");
    }
}
