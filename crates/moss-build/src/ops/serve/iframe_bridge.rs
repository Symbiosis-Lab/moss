//! iframe Bridge - Cross-origin communication between Tauri parent and preview iframe
//!
//! This module provides a JavaScript bridge script that enables:
//! 1. Dynamic RPC calls from parent window to iframe (any window property/method)
//! 2. Navigation state reporting from iframe to parent (URL, canGoBack, canGoForward)
//! 3. External link interception (workaround for Tauri issue #9912)
//!
//! The script is injected into HTML responses served by the preview server,
//! avoiding cross-origin security errors that would occur if injected from parent.
//! A preview-only CSS tooltip (`::after` on `:hover`/`:focus-visible`) is also
//! injected on the comment send button and subscribe button to indicate that
//! submissions stay local. The comment form is rerouted to the local stub
//! (see `preview::comment_stub`).

use axum::{
    body::Body,
    extract::Request,
    http::{header, Response, StatusCode},
    middleware::Next,
    response::IntoResponse,
};
use http_body_util::BodyExt;

/// Bridge script loaded from separate JS file for editor support
const IFRAME_BRIDGE_SCRIPT: &str = include_str!("js/iframe-bridge.js");

/// Preview-only blueprint placeholder for a file that isn't there yet.
///
/// Built from the desktop app's asset-placeholder bridge script. Injected into `<head>`, not
/// before `</body>` like everything else in this module: it registers a
/// capture-phase `error` listener, and an image that fails while the parser is
/// still working through the body would fire before a body-end script existed.
///
/// PREVIEW ONLY, deliberately. A published site never carries this, because a
/// published site can never contain a broken file — moss refuses to deploy one.
/// The placeholder is a working state, and the work happens locally.
const ASSET_PLACEHOLDER_SCRIPT: &str = include_str!("js/asset-placeholder.js");

/// Marker outline for a placeheld image, so it reads as "moss is standing in
/// for something" rather than as a photo of graph paper. Lives here rather than
/// in `site.css` for the same reason the script does: it is preview-only.
const ASSET_PLACEHOLDER_STYLE: &str = "\
.moss-img-fallback{outline:1px solid rgba(20,60,130,.25);outline-offset:-1px}\
[data-theme=\"dark\"] .moss-img-fallback{outline-color:rgba(90,155,255,.3)}";

/// Preview-only shim that reroutes the comment form to the local stub endpoint
/// (see `preview::comment_stub`). Injected into every HTML response alongside
/// the bridge script. Never present in the published artifact — this file is
/// only served by the preview server.
const COMMENT_PREVIEW_SHIM: &str = include_str!("js/comment-preview-shim.js");

/// Preview-only JS that decorates each comment `<li>` with a hover-revealed
/// Hide button and posts `{type:"moss-hide-comment", source, id, pageKey,
/// siteName}` to `window.parent` on click. Injected ONLY when `shell_mounted`
/// — NEVER present in deployed output.
const COMMENT_OWNER_CONTROLS_SCRIPT: &str =
    include_str!("js/comment-owner-controls.js");

/// Preview-only CSS for the owner Hide button. Attribute-selector scoped
/// (`[data-moss-hide-comment]`) so it never collides with site CSS.
/// Uses `inset-inline-end` (RTL-safe) and design tokens. Injected ONLY
/// when `shell_mounted` — NEVER present in deployed output.
const COMMENT_OWNER_CONTROLS_STYLE: &str = "\
li.comment-item{position:relative}\
li.comment-item [data-moss-hide-comment]{position:absolute;top:.25rem;inset-inline-end:.25rem;\
opacity:0;transition:opacity .12s ease;font:inherit;font-size:.75rem;line-height:1;\
padding:.2rem .45rem;cursor:pointer;border-radius:4px;\
color:var(--moss-color-muted,#716d69);\
background:var(--moss-color-surface,#fff);border:1px solid var(--moss-border-light,#ddd)}\
li.comment-item:hover [data-moss-hide-comment],\
li.comment-item [data-moss-hide-comment]:focus-visible{opacity:1}";

/// Query parameter the shell stamps onto preview-iframe URLs so the server
/// knows the document is mounted directly inside the moss app shell (not a
/// nested cover/embed iframe loaded by previewed content). When present, the
/// middleware adds `class="moss-shell-frame"` to `<html>` server-side, so the
/// 48px chrome clearance is in place before first paint and there is no
/// visible jump after the bridge script runs.
///
/// The marker is propagated by the bridge across in-iframe link clicks (see
/// the link interceptor in `iframe-bridge.ts`). Nested `<iframe src="...">`
/// loads from previewed pages never carry the marker because the bridge has
/// no chance to mutate them — so they don't get the class server-side and
/// chrome geometry can't leak into them. The bridge's topology check stays
/// in place as defense-in-depth (handles `window.location = "..."` from
/// user-page JS and other navigation paths the bridge can't intercept).
const SHELL_MARKER: &str = "__moss_shell";

/// The first `max` bytes of `s`, backed off to the nearest char boundary so a
/// multi-byte character straddling the cut is dropped whole rather than split.
fn head_bytes(s: &str, max: usize) -> &str {
    let mut limit = s.len().min(max);
    while limit > 0 && !s.is_char_boundary(limit) {
        limit -= 1;
    }
    s.get(..limit).unwrap_or(s)
}

/// Language bucket for preview-only UI copy, detected from `<html lang>`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum PreviewLang {
    En,
    ZhHans,
    ZhHant,
}

/// Detect the page's language bucket from the opening `<html lang=...>` tag.
/// Bounded scan of the first 512 bytes — the attribute is always near the top
/// of a well-formed document. The value is read only up to its closing quote so
/// a later `hant`/`tw` substring elsewhere in the page cannot leak in.
fn detect_page_lang(html: &str) -> PreviewLang {
    let head = head_bytes(html, 512);
    let lang = head
        .split_once(" lang=\"")
        .map(|(_, rest)| (rest, '"'))
        .or_else(|| head.split_once(" lang='").map(|(_, rest)| (rest, '\'')))
        .and_then(|(rest, quote)| rest.split_once(quote).map(|(value, _)| value));
    match lang {
        // allow:zh_bucket detects page lang attr to select CJK preview copy
        Some(v) if v.to_ascii_lowercase().starts_with("zh") => {
            let lc = v.to_ascii_lowercase();
            if lc.contains("hant") || lc.contains("tw") {
                PreviewLang::ZhHant
            } else {
                PreviewLang::ZhHans
            }
        }
        _ => PreviewLang::En,
    }
}

/// Preview-only `<style>` that renders the "local preview" hint as a hover
/// tooltip on the comment send button and the subscribe button. Injected by the
/// preview server only — NEVER present in published output. Pure CSS (`::after`
/// on hover/focus): no DOM mutation, no JS, so it survives idiomorph morphs
/// (re-injected on every served response) without needing re-application.
///
/// The bubble background/text use `--moss-color-text` / `--moss-color-bg`, which
/// invert with `[data-theme]`, giving good contrast in both light and dark.
fn build_preview_tooltip_style(lang: PreviewLang) -> String {
    let (comment, subscribe) = match lang {
        PreviewLang::ZhHans => (
            "本地预览·评论不会发送到服务器",
            "本地预览·订阅不会发送到服务器",
        ),
        PreviewLang::ZhHant => (
            "本地預覽·評論不會發送到伺服器",
            "本地預覽·訂閱不會發送到伺服器",
        ),
        PreviewLang::En => (
            "Local preview · comment won't be sent",
            "Local preview · subscription won't be sent",
        ),
    };
    format!(
        "<style id=\"moss-preview-tooltip\">\
.comment-form-submit{{position:relative}}\
.comment-form-slot{{overflow:visible}}\
.moss-btn-slot{{position:relative}}\
.comment-form-submit::after,.moss-subscribe-form .moss-btn-slot::after{{\
position:absolute;right:0;bottom:calc(100% + 8px);\
width:max-content;max-width:min(260px,80vw);box-sizing:border-box;\
padding:6px 10px;border-radius:8px;\
background:var(--moss-color-text,#2c2825);color:var(--moss-color-bg,#faf8f5);\
font-size:0.75rem;line-height:1.3;font-weight:400;white-space:normal;text-align:left;\
box-shadow:0 4px 12px rgba(0,0,0,0.18);pointer-events:none;z-index:50}}\
.comment-form-submit:hover::after,.comment-form-submit:focus-visible::after{{content:\"{comment}\"}}\
.moss-subscribe-form .moss-btn-slot:hover::after,.moss-subscribe-form .moss-btn-slot:focus-within::after{{content:\"{subscribe}\"}}\
</style>\n"
    )
}

/// Preview-only `<style>` that makes divider-drag / window-resize reflow cheap on
/// image-heavy pages. The preview iframe resizes on every `--preview-width-inset`
/// write during a divider drag; without this an image-heavy page's FULL relayout
/// stalled the shell main thread 600ms+ per frame (measured). `content-visibility:
/// auto` lets off-screen media skip layout, so a width change only relays out the
/// on-screen media, not every figure.
///
/// Scoped to MEDIA ONLY (figures, bare images, embeds) — text stays fully laid out
/// so find-in-page (`window.find`) and editor↔preview scroll-sync (which key off
/// text `[data-source-line]` anchors) are unaffected. `contain-intrinsic-size: auto
/// <fallback>` reserves a placeholder height (WebKit refines it after first render)
/// so off-screen media don't collapse the fake scrollbar's reported height.
///
/// The size is written in the TWO-VALUE form `auto none auto 600px` = `<width>
/// <height>`: height keeps the 600px placeholder, width claims NO intrinsic size.
/// The one-value form (`auto 600px`) applies to BOTH axes, which made every
/// off-screen figure report a 600px intrinsic WIDTH. Inside a `.moss-grid`, whose
/// tracks are `repeat(N, 1fr)` = `minmax(auto, 1fr)`, that width became each
/// track's automatic minimum — a 3-column grid laid out at 600px per column and
/// overflowed the content column until the figure scrolled into view and snapped
/// back. Height reservation is what this rule needs; width never was.
///
/// Preview-only: injected by the preview server, NEVER in published output. Pure
/// CSS → survives idiomorph morphs (re-injected on every served response).
/// `content-visibility` is recent WebKit (Safari 18+); older engines ignore the
/// declaration → graceful no-op, no regression.
const PREVIEW_CHEAP_REFLOW_STYLE: &str = "<style id=\"moss-preview-cheap-reflow\">\
article figure.moss-image,\
article p:has(> img),\
article p:has(> picture),\
article .moss-embed{\
content-visibility:auto;contain-intrinsic-size:auto none auto 600px}\
</style>\n";

/// Preview-only `<style>` defining the one-shot wash that answers "where did my
/// footer go?" when the user opens a slot file (`footer.md`) in the editor.
/// `footer.md` becomes no page of its own, so the editor cannot navigate to it;
/// instead the shell scrolls the current page's `<footer>` into view and runs
/// this animation once (see `NavigationManager.revealFooter`).
///
/// The keyframes live here, server-side, rather than as inline styles pushed
/// over RPC: an animation is declarative, so revealing is one `animation` write
/// and one clear, instead of a host-side timer chain nudging box-shadow values.
///
/// `background-color` (not `outline` / `box-shadow`) because the footer is a
/// full-bleed band — a wash reads as "this region", where a ring around a
/// page-wide element reads as a rendering glitch. `--moss-color-accent-quiet`
/// is theme-aware, so the wash stays legible in dark mode.
///
/// `.moss-fm-flash` is the hover-driven strength of the same wash: hovering a
/// chip in the property bar flashes the element(s) the field renders as.
/// Class-based rather
/// than an inline `animation` write because hover needs a clean clear on
/// hover-out and a reduced-motion form: under `prefers-reduced-motion` the
/// animation is dropped and the class holds a static tint for as long as the
/// hover lasts (same jump-cut convention as `progress.css`).
///
/// Preview-only: injected by the preview server, NEVER in published output.
/// Pure CSS → survives idiomorph morphs (re-injected on every served response).
const PREVIEW_SLOT_REVEAL_STYLE: &str = "<style id=\"moss-preview-slot-reveal\">\
@keyframes moss-slot-reveal{\
from{background-color:var(--moss-color-accent-quiet)}\
to{background-color:transparent}}\
.moss-fm-flash{animation:moss-slot-reveal 1400ms ease-out}\
@media (prefers-reduced-motion:reduce){\
.moss-fm-flash{animation:none;background-color:var(--moss-color-accent-quiet)}\
}\
</style>\n";

/// Build the CSS `<style>` tag injected into every previewed page.
///
/// Chrome and content live in separate layers. The iframe document paints
/// edge-to-edge in its own coordinate system; the parent shell paints the
/// titlebar (translucent overlay) and the fake scrollbar over it. The
/// iframe must NOT carry chrome geometry (no body padding, no fake
/// scrollbar element) by default. The only iframe-side concerns are:
///
///   - Hide the native scrollbar. Only the iframe can hide its own
///     scrollbar — and we must, because an unhidden scrollbar would peek
///     under the translucent titlebar.
///   - Transparent html background so the iframe element's own background
///     (set in shell CSS) shows through, avoiding WKWebView's white
///     default.
///
/// # Chrome clearance: owned by the shell
///
/// The preview iframe is inset below the floating titlebar by the shell
/// (`#moss-preview-iframe { top: var(--moss-titlebar-height) }`), so the
/// previewed document never paints under the chrome and carries NO
/// clearance of its own — no body padding, no `scroll-padding-top`. The
/// served document is byte-identical for a preview and a real web visit.
///
/// The `moss-shell-frame` class described below is still injected but is
/// now **inert**: `site.css` defines no rules for it. It is retained so
/// this change stays trivially revertable; removing the marker middleware
/// and the topology check is follow-up work. The historical
/// two-mechanism design was:
///
///   1. **Server-side, before first paint (primary path):** when the
///      request URL carries the `__moss_shell` query param (the SHELL_MARKER
///      const), the middleware below rewrites the served `<html>` tag to
///      include `class="moss-shell-frame"`. The browser's HTML parser sees
///      the class in the document's first stylesheet recalc, so the body
///      is laid out with the 48 px padding from the very first frame.
///      No flash, no jump.
///
///   2. **Client-side, defense-in-depth (fallback path):** the bridge
///      script (`iframe-bridge.ts`) runs a topology check
///      `window !== window.top && window.parent === window.top` on every
///      load. If the topology says we're a direct child of the shell and
///      the class isn't already there (e.g. some user-page JS did
///      `window.location = "/foo"` without going through the bridge's link
///      interceptor, so the marker was lost), the script adds the class.
///      This causes a one-frame flash but recovers correctness on the
///      next stylesheet recalc. Conversely, if the class IS present but
///      topology says we're inside a nested iframe (defensive against a
///      hypothetical bug where the marker leaked into a nested URL), the
///      script removes the class.
///
/// # Marker propagation
///
/// The shell stamps `__moss_shell=1` onto every iframe URL it constructs
/// (see the desktop app's shell-marker module). The bridge's link-click
/// interceptor re-stamps the marker when navigating internally (see the
/// link interceptor in `iframe-bridge.ts`). Together this covers ~99% of
/// in-iframe navigation paths: link clicks, programmatic Navigation API
/// usage, popstate. The remaining edge case (raw `window.location =`
/// from user-page JS) is handled by the bridge's fallback path above.
///
/// # Nested iframes (chrome leak guard)
///
/// Nested `<iframe src="./sketch.html">` loads from previewed content
/// (e.g. p5 sketches in a site's interactive pages) bypass the bridge entirely
/// when constructing the URL — the browser fires the request directly
/// from the parent document's parser. So the marker is never present on
/// the nested URL, the middleware doesn't add the class, and the bridge's
/// topology check (running inside the nested iframe) confirms it's not
/// shell-mounted and skips the class-add path. **Result: nested iframes
/// have no class, no padding, no flash, no inverse-flash. Chrome geometry
/// cannot leak in.** This was the load-bearing guarantee that motivated
/// the topology check originally; it survives intact.
///
/// # Real-browser parity
///
/// A real-browser visit (e.g. a site served from GitHub Pages) never
/// hits this middleware and never loads the bridge script. The HTML is
/// served byte-identical to what the moss build wrote out, so the
/// served-on-web layout is unchanged. (The `__moss_shell` query param is
/// stamped only by the moss shell, never by published links.)
///
/// # No chrome-height constant here
///
/// This used to emit `scroll-padding-top: 48px` so anchor targets cleared the
/// floating titlebar, duplicating `system::utils::TITLEBAR_HEIGHT` and the
/// `.moss-shell-frame body { padding-top }` literal in `site.css` (history:
/// 48 → 38 → 52 → 48). The shell now insets the preview iframe below the
/// chrome, so the iframe's viewport top *is* the first visible row: an anchor
/// scrolled to y=0 lands fully visible, and scroll padding would push it down
/// by a titlebar's height for no reason. The constant lives in exactly one
/// place again — do not reintroduce it here.
///
/// `site.css` does set a root `scroll-padding-top`, and that is a different
/// constant for a different piece of chrome: the site's own floating nav
/// island, which renders inside the preview exactly as it does on the
/// published site. The rule this comment forbids is duplicating the SHELL's
/// titlebar height here; reserving room for chrome the page itself paints is
/// the page's business, and belongs in the site stylesheet.
fn build_style_tag() -> String {
    "<style>\
html{background:transparent!important;scrollbar-width:none}\
html::-webkit-scrollbar,body::-webkit-scrollbar{display:none;width:0;height:0}\
</style>\n"
        .to_string()
}

/// True if the request URL carries the shell marker query param (any value).
///
/// We check presence of the key only, not its value, so the marker is robust
/// to URL fragments that might mangle the value (e.g. `__moss_shell=&foo=bar`).
fn has_shell_marker(query: Option<&str>) -> bool {
    let Some(q) = query else { return false };
    q.split('&')
        .any(|pair| pair == SHELL_MARKER || pair.starts_with(&format!("{SHELL_MARKER}=")))
}

/// Insert `moss-shell-frame` into the served document's opening `<html>` tag.
///
/// Returns the rewritten HTML, or the original if no `<html` tag is found
/// within a sane window near the start of the document. The class is added
/// in one of two ways:
///
///   - If the tag has no existing `class` attribute: insert
///     ` class="moss-shell-frame"` immediately after `<html`.
///   - If the tag already has a `class` attribute (single or double quoted):
///     append ` moss-shell-frame` inside the existing quotes.
///
/// Edge cases:
///
///   - No `<html>` tag at all (parser-synthesized): we return the HTML
///     unchanged. The bridge script's topology check picks up the slack.
///   - `<html>` inside a comment near the top: extremely rare; the cost of
///     a false-positive rewrite is one extra class attribute on a commented-out
///     tag (no effect). We don't try to parse comments.
///   - Mixed-case tag (`<HTML>`): treated as a miss. Modern HTML serializers
///     emit lowercase; a non-match here means we fall back to the JS topology
///     check, same as missing-`<html>`.
fn inject_shell_frame_class(html: &str) -> String {
    // Bound the tag search to the first 4 KB — the `<html>` tag is always at
    // the top of the document, and bounding prevents pathological scans on
    // huge pages with no `<html>` tag (e.g. fragments).
    let Some(tag_start) = head_bytes(html, 4096).find("<html") else {
        return html.to_string();
    };
    let after_html = tag_start + "<html".len();
    // Split the document into "everything through `<html`" and the rest, then
    // carve the opening tag's attribute region off the front of the rest.
    let (Some(before), Some(rest)) = (html.get(..after_html), html.get(after_html..)) else {
        return html.to_string();
    };
    let Some((tag_inner, after_tag)) = rest.split_once('>') else {
        return html.to_string();
    };

    // Look for an existing class attribute. We accept `class="..."` and
    // `class='...'`; bare-word `class=foo` and missing-quote forms are not
    // emitted by any sane HTML serializer and we don't try to handle them.
    let new_inner = match find_class_attr(tag_inner) {
        Some(pos) => {
            // `pos` points at `class=`; the char right after `=` is the quote,
            // and the value runs to the next occurrence of that same quote. A
            // missing closing quote means a malformed tag — bail.
            let after_eq = pos + "class=".len();
            let (Some(attr_head), Some(attr_tail)) =
                (tag_inner.get(..after_eq), tag_inner.get(after_eq..))
            else {
                return html.to_string();
            };
            let mut quoted = attr_tail.chars();
            let Some(quote) = quoted.next() else {
                return html.to_string();
            };
            let Some((value, tail)) = quoted.as_str().split_once(quote) else {
                return html.to_string();
            };
            // Empty existing class (`class=""`)? Don't double-space.
            let separator = if value.is_empty() { "" } else { " " };
            format!("{attr_head}{quote}{value}{separator}moss-shell-frame{quote}{tail}")
        }
        // No existing class attribute. Insert one right after `<html`.
        None => format!(" class=\"moss-shell-frame\"{tag_inner}"),
    };
    format!("{before}{new_inner}>{after_tag}")
}

/// Locate the start offset of a `class="..."` or `class='...'` attribute
/// inside the inner-tag string (everything between `<html` and `>`). Returns
/// `None` if no such attribute is found. Matches must be preceded by
/// whitespace (or be at start) so we don't false-match e.g. `data-class=`.
///
/// Edge case: the boundary check uses byte indexing on a UTF-8 string, so
/// when `class=` is preceded by a multi-byte char (e.g. an exotic
/// `data-名="x" class="..."`), `bytes[i-1]` lands on a UTF-8 continuation
/// byte. `is_ascii_whitespace()` returns false on such bytes (correct: a
/// continuation byte is not whitespace), so we miss the match and the
/// caller falls through to the "insert new class" branch. Result: the
/// element ends up with TWO `class` attributes. Per HTML spec the browser
/// honors only the first, so this is harmless but cosmetically odd. Not
/// worth a heavier scanner since template `<html>` tags don't use
/// non-ASCII attribute names in practice.
fn find_class_attr(tag_inner: &str) -> Option<usize> {
    let bytes = tag_inner.as_bytes();
    // Strict `<` so the `bytes[i + "class=".len()]` access at the body of
    // the loop is always in-bounds. (We need at least one byte AFTER
    // `class=` to inspect the following quote.)
    let mut i = 0;
    while i + "class=".len() < bytes.len() {
        if &bytes[i..i + "class=".len()] == b"class=" {
            // Must be preceded by whitespace or start-of-tag.
            let preceded_ok = i == 0 || bytes[i - 1].is_ascii_whitespace();
            // Must be followed by a quote.
            let after = bytes[i + "class=".len()];
            if preceded_ok && (after == b'"' || after == b'\'') {
                return Some(i);
            }
        }
        i += 1;
    }
    None
}

/// Rewrite the baked comment form's server URL to the preview stub.
///
/// PRIMARY safety mechanism for comment preview isolation. The preview refresh path
/// uses idiomorph in-place morphing: it fetches fresh HTML from the preview server
/// and syncs attributes onto the preserved `#moss-comment-form` node. If the
/// SERVED bytes carry the production URL, one watch-rebuild reverts the form to
/// posting live — the JS shim's attribute rewrite is undone by the morph sync.
///
/// By rewriting here (server-side), every served HTML response carries
/// `/__moss/comments` as the form's server URL regardless of what was baked into
/// the artifact. The preview-shim.ts end-of-body rewrite remains in place as
/// defense-in-depth for paths that bypass this middleware.
///
/// Inactive forms (empty server_url — the form is rendered with
/// `class="moss-service-inactive"` when baked without a service URL) also get
/// rewritten. This is safe and harmless: the inactive state is decided at BAKE time
/// via the `moss-service-inactive` class on the `<section>`, not the attribute.
/// Rewriting the attribute on an inactive form does not make the form active —
/// `artalk.ts` reads and submits the URL as a fetch destination, but the section's
/// inactive class is set at render time and not re-examined during submission.
/// Verified by reading `render_comment_section` in `build/features/comment/render.rs`:
/// the `active` flag controls which section tag is emitted (with or without
/// `moss-service-inactive`), and once baked that class does not change at runtime.
///
/// Only one form per page is expected (single `#moss-comment-form` id). If somehow
/// multiple forms are present, only the first occurrence is rewritten (the `find`
/// returns the first match). This is consistent with how the artalk client binds —
/// it also looks up `getElementById("moss-comment-form")` which returns the first.
fn rewrite_comment_form_server_url(html: &str) -> String {
    const FORM_MARKER: &str = "id=\"moss-comment-form\" data-server-url=\"";
    let Some((before, rest)) = html.split_once(FORM_MARKER) else {
        return html.to_string();
    };
    // The baked value runs to its closing quote; keep the quote and everything
    // after it, replacing only the value itself.
    let Some((_baked, after)) = rest.split_once('"') else {
        return html.to_string();
    };
    format!("{before}{FORM_MARKER}/__moss/comments\"{after}")
}

/// Remove `<!--moss:no-preview-->…<!--/moss:no-preview-->` regions from served
/// preview HTML. The published artifact keeps these (mode-independent); only
/// the preview origin must not fire foreign analytics on every reload.
fn strip_preview_only_scripts(html: &str) -> String {
    const OPEN: &str = "<!--moss:no-preview-->";
    const CLOSE: &str = "<!--/moss:no-preview-->";
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some((before, after_open)) = rest.split_once(OPEN) {
        out.push_str(before);
        match after_open.split_once(CLOSE) {
            Some((_region, after_close)) => rest = after_close,
            None => { rest = after_open; break; } // unbalanced: drop the marker, keep going
        }
    }
    out.push_str(rest);
    out
}


/// Guarantee `data-moss-preview` on the served page's `<body>` tag.
///
/// Staging builds annotate the attribute at build time (`pipeline.rs`), but
/// the zero-flicker mechanism serves the previous FROZEN generation during
/// rebuilds and on cold start — and `ship_phase` strips the attribute from
/// frozen pages. Every runtime preview gate (subscribe.ts's no-real-POST
/// gate, the beacon's defense-in-depth check) is blind on such pages, so
/// the server re-guarantees the attribute on every HTML response it serves,
/// independent of which generation backs it.
///
/// No-ops when the attribute is already present or no `<body>` tag exists
/// (fragments). Mirrors `inject_shell_frame_class`'s string-rewrite approach.
///
/// The scan is anchored past `</head>` when one exists: head content can
/// legitimately contain a literal `<body` — `escape_json_string` does not
/// escape angle brackets, so an article titled `Styling the <body> element`
/// puts one into the JSON-LD block — and a first-substring-wins scan from
/// offset 0 would inject into that string and leave the real tag ungated.
/// Residual limitation (accepted): a head string containing `</head>` could
/// still misanchor; no moss-emitted head content produces that.
fn ensure_preview_body_attr(html: &str) -> String {
    let anchor = html.find("</head>").map_or(0, |p| p + "</head>".len());
    let (Some(prefix), Some(scan)) = (html.get(..anchor), html.get(anchor..)) else {
        return html.to_string();
    };
    for (pos, marker) in scan.match_indices("<body") {
        let Some(tail) = scan.get(pos + marker.len()..) else {
            continue;
        };
        // Require a real tag boundary so `<bodyguard>` doesn't match.
        if !tail.starts_with(['>', ' ', '\t', '\n', '\r', '/']) {
            continue;
        }
        let Some((attrs, _)) = tail.split_once('>') else {
            return html.to_string(); // malformed open tag: leave unchanged
        };
        if attrs.contains("data-moss-preview") {
            return html.to_string();
        }
        let Some(before) = scan.get(..pos + marker.len()) else {
            return html.to_string();
        };
        return format!("{prefix}{before} data-moss-preview{tail}");
    }
    html.to_string()
}

/// Inject all preview-only assets (style, tooltip, bridge script, comment shim,
/// and — when `shell_mounted` — the owner-controls Hide button) before `</body>`.
///
/// This is a pure function so it can be unit-tested without spinning up axum.
/// The middleware calls it after all the server-side HTML rewrites are done.
///
/// # Invariant
///
/// The owner-controls `<style>` and `<script>` are ONLY emitted when
/// `shell_mounted` is `true`. Deployed output never carries the shell marker,
/// so the controls are never present in published artifacts.
fn inject_preview_assets(html: &str, shell_mounted: bool) -> String {
    let Some((before, after)) = html.rsplit_once("</body>") else {
        // No </body> — return unchanged (mirrors the middleware's fallback).
        return html.to_string();
    };

    let style_tag = build_style_tag();
    let tooltip_style = build_preview_tooltip_style(detect_page_lang(html));
    let script_tag = format!(
        "<script type=\"text/javascript\" id=\"moss-bridge\" data-moss-permanent>\n{}\n</script>\n",
        IFRAME_BRIDGE_SCRIPT
    );
    let shim_tag = format!(
        "<script type=\"text/javascript\" id=\"moss-comment-preview-shim\" data-moss-permanent>\n{}\n</script>\n",
        COMMENT_PREVIEW_SHIM
    );

    let owner_controls = if shell_mounted {
        format!(
            "<style id=\"moss-comment-owner-style\">{COMMENT_OWNER_CONTROLS_STYLE}</style>\
             <script type=\"text/javascript\" id=\"moss-comment-owner-controls\" data-moss-permanent>{COMMENT_OWNER_CONTROLS_SCRIPT}</script>"
        )
    } else {
        String::new()
    };
    format!(
        "{before}{style_tag}{tooltip_style}{PREVIEW_CHEAP_REFLOW_STYLE}{PREVIEW_SLOT_REVEAL_STYLE}\
         {script_tag}{shim_tag}{owner_controls}</body>{after}"
    )
}

/// Inject the blueprint placeholder immediately after the opening `<head>`.
///
/// Everything else this module injects goes before `</body>`; this one cannot.
/// The placeholder listens for image `error` events, and an image can fail
/// while the parser is still in the body — before a body-end script has run.
/// Placing it first inside `<head>` means the listener exists before the
/// document's first image request is even issued.
///
/// A malformed document with no `<head>` is returned unchanged, matching how
/// `inject_preview_assets` treats a missing `</body>`.
fn inject_placeholder_into_head(html: &str) -> String {
    let Some(open) = html.find("<head") else {
        return html.to_string();
    };
    let Some(rel_end) = html.get(open..).and_then(|s| s.find('>')) else {
        return html.to_string();
    };
    let insert_at = open + rel_end + 1;
    let (Some(before), Some(after)) = (html.get(..insert_at), html.get(insert_at..)) else {
        return html.to_string();
    };
    format!(
        "{before}<style id=\"moss-img-fallback-style\">{ASSET_PLACEHOLDER_STYLE}</style>\
         <script id=\"moss-img-fallback\" data-moss-permanent>{ASSET_PLACEHOLDER_SCRIPT}</script>{after}"
    )
}

/// Axum middleware that injects the iframe bridge script into HTML responses
///
/// This middleware:
/// 1. Checks if the response is HTML (Content-Type: text/html)
/// 2. Reads the response body
/// 3. Injects the script before the closing </body> tag
/// 4. Returns the modified response
pub async fn inject_iframe_bridge(
    request: Request,
    next: Next,
) -> Result<impl IntoResponse, StatusCode> {
    // Capture whether the shell marker is present BEFORE consuming the
    // request — we use it later to decide whether to add the
    // `moss-shell-frame` class server-side. See SHELL_MARKER docs.
    let shell_mounted = has_shell_marker(request.uri().query());

    // Get the response from the next middleware/handler
    let response = next.run(request).await;

    // Check if this is an HTML response
    let is_html = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.starts_with("text/html"))
        .unwrap_or(false);

    // If not HTML, return response as-is
    if !is_html {
        return Ok(response);
    }

    // Extract response parts
    let (parts, body) = response.into_parts();

    // Read the body
    let bytes = match body.collect().await {
        Ok(collected) => collected.to_bytes(),
        Err(_) => {
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    // Convert to string
    let html = match String::from_utf8(bytes.to_vec()) {
        Ok(s) => s,
        Err(_) => {
            // If not valid UTF-8, return the original bytes
            return Ok(Response::from_parts(parts, Body::from(bytes)));
        }
    };

    // Step 1: rewrite the baked comment form's server URL to the preview stub.
    // This is the PRIMARY safety mechanism for comment preview isolation — the
    // preview refresh path uses idiomorph in-place morphing, which syncs
    // attributes from freshly-fetched HTML onto the live DOM. If served bytes
    // carry the production URL, one watch-rebuild reverts the form to posting
    // live. Server-side rewrite ensures every served response carries the stub
    // URL. The preview-shim.ts rewrite stays as defense-in-depth.
    let html = rewrite_comment_form_server_url(&html);

    // Step 1b: strip <!--moss:no-preview-->…<!--/moss:no-preview--> regions.
    // The published artifact keeps them (analytics script is mode-independent);
    // the preview origin must not fire foreign analytics on every reload.
    let html = strip_preview_only_scripts(&html);

    // Step 1c: guarantee data-moss-preview on <body>. The zero-flicker window
    // serves ship-stripped frozen generations where the build-time annotation
    // is gone; runtime preview gates (subscribe.ts, beacon) depend on it.
    let html = ensure_preview_body_attr(&html);

    // Step 2: if the shell marker is present, server-rewrite the `<html>`
    // tag to carry `class="moss-shell-frame"` BEFORE first paint. Without
    // this, the browser would parse and paint the body at top:0, then the
    // bridge script (loaded before </body>) would add the class and the
    // body would jump down 48px — visible flash on every navigation. With
    // the marker, the class is in the parser's first stylesheet recalc.
    //
    // Nested `<iframe src>` loads from previewed pages never carry the
    // marker (the bridge has no chance to mutate them), so they don't get
    // the class server-side. The bridge's topology check then guarantees
    // the class also doesn't get added at runtime in nested frames.
    let html = if shell_mounted {
        inject_shell_frame_class(&html)
    } else {
        html
    };

    // Inject minimal iframe-side CSS, preview tooltip, bridge script, comment
    // shim, and (when shell_mounted) the owner-controls Hide button, before
    // </body>. See `inject_preview_assets` for the full injection logic and
    // the shell-marker gate that keeps owner controls out of deployed output.
    let html = inject_preview_assets(&html, shell_mounted);

    // Last: the blueprint placeholder, into <head> rather than before </body>
    // (see `inject_placeholder_into_head`). Preview-only — a published site
    // cannot contain a broken file, so it has nothing to placeholder.
    let injected_html = inject_placeholder_into_head(&html);

    // Remove Content-Length header since we modified the body
    let mut parts = parts;
    parts.headers.remove(header::CONTENT_LENGTH);

    // Prevent browser caching to ensure fresh content on each navigation
    // This is critical for preview mode where content changes frequently
    parts.headers.insert(
        header::CACHE_CONTROL,
        "no-cache, no-store, must-revalidate".parse().unwrap(),
    );

    // Create new response with modified body
    Ok(Response::from_parts(parts, Body::from(injected_html)))
}

#[cfg(test)]
#[path = "iframe_bridge_tests.rs"]
mod tests;
