use super::*;

#[test]
fn test_script_contains_rpc_handler() {
    assert!(IFRAME_BRIDGE_SCRIPT.contains("moss-rpc-call"));
    assert!(IFRAME_BRIDGE_SCRIPT.contains("moss-rpc-result"));
    // Explicit dispatch uses switch/case - check for method strings (survives minification)
    assert!(
        IFRAME_BRIDGE_SCRIPT.contains("history.back"),
        "RPC handler should include history.back method"
    );
}

#[test]
fn test_injects_preview_only_cheap_reflow_style() {
    let html =
        "<html><body><article><figure class=\"moss-image\"><img/></figure></article></body></html>";
    let out = inject_preview_assets(html, false);
    // Injected into the preview response…
    assert!(out.contains("moss-preview-cheap-reflow"));
    assert!(out.contains("content-visibility:auto"));
    assert!(out.contains("contain-intrinsic-size"));
    // …scoped to MEDIA blocks only (text stays laid out → find-in-page +
    // editor↔preview scroll-sync unaffected).
    assert!(out.contains("figure.moss-image"));
    assert!(
        !out.contains("article section"),
        "must not target text sections"
    );
    // …and NEVER present in the source/published HTML (preview-only injection).
    assert!(!html.contains("moss-preview-cheap-reflow"));
}

#[test]
fn test_injects_slot_reveal_keyframes() {
    // The shell reveals a slot file's footer by writing one `animation`
    // property over RPC (`NavigationManager.revealFooter`). That write is
    // inert unless the keyframes it names are already in the document, and
    // they must be re-injected on every response so an idiomorph morph
    // cannot drop them.
    let html = "<html><body><footer class=\"container\"></footer></body></html>";
    let out = inject_preview_assets(html, false);
    assert!(out.contains("@keyframes moss-slot-reveal"));
    // The chip-hover flash rides the same wash as a class (`.moss-fm-flash`,
    // toggled by NavigationManager.flashSource over RPC), with a static-tint
    // reduced-motion form. Both rules must ship with every response too.
    assert!(out.contains(".moss-fm-flash{animation:moss-slot-reveal"));
    assert!(out.contains("prefers-reduced-motion"));
    // Preview-only: never in the source the published pipeline ships.
    assert!(!html.contains("moss-slot-reveal"));
}

#[test]
fn test_cheap_reflow_reserves_height_only_never_width() {
    // Regression: the one-value form `contain-intrinsic-size: auto 600px`
    // applies to BOTH axes, so every off-screen figure claimed a 600px
    // intrinsic WIDTH. `.moss-grid` tracks are `repeat(N, 1fr)` =
    // `minmax(auto, 1fr)`, whose automatic minimum is the item's min-content
    // contribution — so a 3-column grid laid out at `600px 600px 600px`
    // inside a ~692px column and overflowed the page, snapping back only
    // once the figure scrolled into view. Published output was never
    // affected, which is why this reproduced ONLY in the preview.
    //
    // The two-value form is `<width> <height>`: width must claim no
    // intrinsic size, height keeps the placeholder that stops off-screen
    // media collapsing the fake scrollbar's reported height.
    let out = inject_preview_assets("<html><body></body></html>", false);
    assert!(
        out.contains("contain-intrinsic-size:auto none auto 600px"),
        "must use the two-value <width> <height> form; got: {out}"
    );
    assert!(
        !out.contains("contain-intrinsic-size:auto 600px"),
        "one-value form applies to BOTH axes and blows out grid tracks"
    );
}

#[test]
fn test_script_has_rpc_allowlist() {
    // Security: RPC handler should only allow specific methods to prevent
    // arbitrary code execution (addresses CodeQL security alert)
    assert!(
        IFRAME_BRIDGE_SCRIPT.contains("history.back"),
        "RPC allowlist should include history.back"
    );
    assert!(
        IFRAME_BRIDGE_SCRIPT.contains("history.forward"),
        "RPC allowlist should include history.forward"
    );
    // The script should reject non-allowlisted methods
    assert!(
        IFRAME_BRIDGE_SCRIPT.contains("not allowed"),
        "RPC handler should reject non-allowlisted methods"
    );
}

#[test]
fn test_script_contains_navigation_reporter() {
    assert!(IFRAME_BRIDGE_SCRIPT.contains("moss-navigation"));
    assert!(IFRAME_BRIDGE_SCRIPT.contains("location.href"));
}

#[test]
fn test_script_contains_external_link_handler() {
    assert!(IFRAME_BRIDGE_SCRIPT.contains("open-external-link"));
    // Check for http:// and https:// URL detection (survives minification)
    assert!(IFRAME_BRIDGE_SCRIPT.contains("http://"));
    assert!(IFRAME_BRIDGE_SCRIPT.contains("https://"));
}

#[test]
fn test_script_suppresses_native_context_menu() {
    // Cut 2 of the context-menu vocabulary: the bridge owns right-click inside the previewed
    // page — it suppresses the native WebKit menu and posts the click's
    // context ("moss-context-menu") for the shell to render. Both strings
    // survive minification.
    assert!(
        IFRAME_BRIDGE_SCRIPT.contains("contextmenu"),
        "iframe-bridge should listen for contextmenu"
    );
    assert!(
        IFRAME_BRIDGE_SCRIPT.contains("moss-context-menu"),
        "iframe-bridge should post the moss-context-menu payload to the shell"
    );
}

#[test]
fn test_script_handles_data_external_attribute() {
    // The script should check for data-external attribute on links
    // This enables relative URLs like /feed.xml to open externally
    assert!(
        IFRAME_BRIDGE_SCRIPT.contains("data-external"),
        "iframe-bridge should handle data-external attribute for RSS links"
    );
}

#[test]
fn test_script_adds_cache_busting_to_internal_links() {
    // The script should intercept internal link clicks and add cache-busting
    // This prevents browser disk cache from serving stale pages without
    // contacting the server, which would prevent bridge script injection
    assert!(
        IFRAME_BRIDGE_SCRIPT.contains("_t"),
        "iframe-bridge should add _t cache-busting parameter to internal links"
    );
    assert!(
        IFRAME_BRIDGE_SCRIPT.contains("Date.now()"),
        "iframe-bridge should use Date.now() for cache-busting timestamp"
    );
}

// Note: keyboard shortcut handler (Cmd+R for refresh) is handled at the
// Tauri/frontend level instead of in the iframe bridge script

#[test]
fn test_script_implements_scroll_coordination_protocol() {
    // The bridge reports scroll geometry to the parent shell, which
    // paints the fake scrollbar. The iframe is the source of scroll
    // truth and accepts scroll-to commands from the shell.
    assert!(
        IFRAME_BRIDGE_SCRIPT.contains("moss-iframe-scroll"),
        "bridge must report scroll geometry via moss-iframe-scroll"
    );
    assert!(
        IFRAME_BRIDGE_SCRIPT.contains("moss-iframe-scroll-to"),
        "bridge must accept moss-iframe-scroll-to commands from shell"
    );
}

#[test]
fn style_tag_carries_no_chrome_geometry() {
    // The injected stylesheet must NOT displace the document body, inject
    // chrome elements, or carry the titlebar height in ANY form. Chrome
    // (titlebar overlay, fake scrollbar) is painted by the parent shell,
    // and the shell also owns the chrome OFFSET by insetting
    // the preview iframe — so the iframe side has no chrome geometry at
    // all. This invariant prevents the class of bug where chrome geometry
    // leaks into nested cover or embed iframes (e.g. a site's embedded p5
    // sketch regression).
    let css = build_style_tag();
    // Reject `padding-top` selectors that would displace content.
    assert!(
        !css.contains("body{padding-top") && !css.contains(" padding-top:"),
        "style tag must NOT set body padding (chrome belongs to shell); got: {css}"
    );
    assert!(
        !css.contains("moss-fake-scrollbar"),
        "style tag must NOT inject fake scrollbar (it lives in shell); got: {css}"
    );
    assert!(
        !css.contains("moss-mobile"),
        "style tag must NOT gate on .moss-mobile (no chrome geometry to gate); got: {css}"
    );
    // With the iframe inset below the chrome, the iframe viewport's
    // top row is already fully visible, so scroll padding would push anchor
    // targets down by a titlebar height for no reason. Asserting its ABSENCE
    // keeps TITLEBAR_HEIGHT from creeping back into a second location.
    assert!(
        !css.contains("scroll-padding-top"),
        "style tag must NOT set scroll-padding-top (shell owns the chrome offset); got: {css}"
    );
    assert!(
        css.contains("scrollbar-width:none"),
        "style tag must hide native scrollbar (only iframe can hide its own); got: {css}"
    );
    // Chrome clearance is applied via a class toggle (`html.moss-shell-frame`)
    // set by the bridge script when the topology check passes, NOT via a
    // CSS variable. The injected style must not declare any chrome-top
    // variable at all.
    assert!(
            !css.contains("--moss-chrome-top"),
            "style tag must NOT declare any --moss-chrome-top variable; clearance is done via .moss-shell-frame class. got: {css}"
        );
}

#[test]
fn test_script_injection() {
    let html = r#"<!DOCTYPE html>
<html>
<head><title>Test</title></head>
<body>
<h1>Hello</h1>
</body>
</html>"#;

    let pos = html.rfind("</body>").unwrap();
    let script_tag = format!(
        "<script type=\"text/javascript\" id=\"moss-bridge\" data-moss-permanent>\n{}\n</script>\n",
        IFRAME_BRIDGE_SCRIPT
    );
    let result = format!("{}{}{}", &html[..pos], script_tag, &html[pos..]);

    assert!(result.contains("moss-rpc-call"));
    assert!(result.contains("id=\"moss-bridge\""));
    assert!(result.contains("data-moss-permanent"));
    assert!(result.contains("</body>"));
    // Script should be before </body>
    let script_pos = result.find("moss-rpc-call").unwrap();
    let body_pos = result.rfind("</body>").unwrap();
    assert!(script_pos < body_pos);
}

#[test]
fn shim_script_injected_before_body_close() {
    // Verify the COMMENT_PREVIEW_SHIM tag is present in the bundle and
    // carries the correct id + data-moss-permanent attrs so idiomorph
    // treats it as a structural fixed point (same contract as moss-bridge).
    let html = r#"<html><body><div id="default-form-slot"></div></body></html>"#;
    let pos = html.rfind("</body>").unwrap();
    let style_tag = build_style_tag();
    let script_tag = format!(
        "<script type=\"text/javascript\" id=\"moss-bridge\" data-moss-permanent>\n{}\n</script>\n",
        IFRAME_BRIDGE_SCRIPT
    );
    let shim_tag = format!(
            "<script type=\"text/javascript\" id=\"moss-comment-preview-shim\" data-moss-permanent>\n{}\n</script>\n",
            COMMENT_PREVIEW_SHIM
        );
    let result = format!(
        "{}{}{}{}{}",
        &html[..pos],
        style_tag,
        script_tag,
        shim_tag,
        &html[pos..]
    );

    assert!(
        result.contains("id=\"moss-comment-preview-shim\""),
        "shim tag must carry id for idiomorph fixed-point matching"
    );
    assert!(
        result.contains("__moss/comments"),
        "shim must contain the stub route path"
    );
    // Shim must appear before </body>
    let shim_pos = result.find("moss-comment-preview-shim").unwrap();
    let body_pos = result.rfind("</body>").unwrap();
    assert!(shim_pos < body_pos, "shim must be injected before </body>");
    // Bridge must appear before shim (ordering: bridge first, shim second)
    let bridge_pos = result.find("id=\"moss-bridge\"").unwrap();
    assert!(
        bridge_pos < shim_pos,
        "bridge script must appear before comment shim"
    );
}

// ====================================================================
// Shell-marker / `moss-shell-frame` server-side injection tests
// ====================================================================
//
// These tests cover the chrome-clearance fix that eliminates the
// first-paint flash where preview content briefly rendered under the
// floating titlebar before the bridge's runtime topology check added
// the class. See the long doc comment on `build_style_tag` for the
// architecture, and `SHELL_MARKER` for the propagation contract.

#[test]
fn shell_marker_query_detection() {
    // Bare key form (`__moss_shell` with no `=value`) and value form
    // (`__moss_shell=1`) both count. We also verify that lookalike
    // params do NOT trigger detection (no `data-__moss_shell`-style
    // false positives — the param is a top-level query key, not a
    // substring match).
    assert!(has_shell_marker(Some("__moss_shell=1")));
    assert!(has_shell_marker(Some("__moss_shell")));
    assert!(has_shell_marker(Some("foo=bar&__moss_shell=1")));
    assert!(has_shell_marker(Some("__moss_shell=1&foo=bar")));
    assert!(has_shell_marker(Some("foo=bar&__moss_shell=&baz=qux")));
    assert!(!has_shell_marker(Some("foo=bar")));
    assert!(!has_shell_marker(Some("not__moss_shell=1")));
    assert!(!has_shell_marker(Some("__moss_shell_other=1")));
    assert!(!has_shell_marker(Some("")));
    assert!(!has_shell_marker(None));
}

#[test]
fn inject_class_into_bare_html_tag() {
    // The most common form for moss-built pages.
    let html = "<!DOCTYPE html><html><head></head><body></body></html>";
    let result = inject_shell_frame_class(html);
    assert_eq!(
        result,
        "<!DOCTYPE html><html class=\"moss-shell-frame\"><head></head><body></body></html>"
    );
}

#[test]
fn inject_class_preserves_other_attributes() {
    // moss templates and most user templates emit `<html lang="...">`.
    // The class must be inserted without disturbing the `lang` attr.
    let html = "<!DOCTYPE html><html lang=\"en\"><head></head><body></body></html>";
    let result = inject_shell_frame_class(html);
    assert_eq!(
            result,
            "<!DOCTYPE html><html class=\"moss-shell-frame\" lang=\"en\"><head></head><body></body></html>"
        );
}

#[test]
fn inject_class_merges_with_existing_double_quoted_class() {
    // Some templates already carry a class on `<html>` (e.g. Modernizr's
    // `no-js` shim, or theme switches). We must merge, not replace.
    let html = "<html class=\"no-js dark\"><body></body></html>";
    let result = inject_shell_frame_class(html);
    assert_eq!(
        result,
        "<html class=\"no-js dark moss-shell-frame\"><body></body></html>"
    );
}

#[test]
fn inject_class_merges_with_existing_single_quoted_class() {
    // Single-quoted attribute values are valid HTML and some serializers
    // emit them. The merge logic must handle both quote styles.
    let html = "<html class='no-js'><body></body></html>";
    let result = inject_shell_frame_class(html);
    assert_eq!(
        result,
        "<html class='no-js moss-shell-frame'><body></body></html>"
    );
}

#[test]
fn inject_class_handles_empty_existing_class() {
    // Edge case: `<html class="">` — don't double-space ("  moss-shell-frame").
    let html = "<html class=\"\"><body></body></html>";
    let result = inject_shell_frame_class(html);
    assert_eq!(
        result,
        "<html class=\"moss-shell-frame\"><body></body></html>"
    );
}

#[test]
fn inject_class_does_not_match_data_class_attribute() {
    // `data-class="..."` must not be matched as a class attribute (the
    // `class=` substring appears inside it). The class= match requires
    // a preceding whitespace boundary.
    let html = "<html data-class=\"foo\"><body></body></html>";
    let result = inject_shell_frame_class(html);
    // Should insert a new class attribute rather than mangling the data attr.
    assert_eq!(
        result,
        "<html class=\"moss-shell-frame\" data-class=\"foo\"><body></body></html>"
    );
}

#[test]
fn inject_class_returns_unchanged_when_no_html_tag() {
    // Some served fragments (e.g. snippets injected by SSGs) have no
    // `<html>` at all — the parser synthesizes one. We bail; the bridge
    // script's topology check picks up the slack.
    let html = "<body><h1>Fragment</h1></body>";
    let result = inject_shell_frame_class(html);
    assert_eq!(result, html);
}

#[test]
fn inject_class_returns_unchanged_when_html_tag_unterminated() {
    // Defensive: malformed `<html` with no `>` — bail rather than panic
    // or produce mangled output.
    let html = "<!DOCTYPE html><html lang=\"en\"";
    let result = inject_shell_frame_class(html);
    assert_eq!(result, html);
}

#[test]
fn inject_class_handles_html_with_lots_of_whitespace() {
    // Templates may emit `<html   lang="en"  data-theme="x">`.
    let html = "<html   lang=\"en\"  data-theme=\"dark\"><body></body></html>";
    let result = inject_shell_frame_class(html);
    assert_eq!(
        result,
        "<html class=\"moss-shell-frame\"   lang=\"en\"  data-theme=\"dark\"><body></body></html>"
    );
}

#[test]
fn inject_class_only_affects_first_html_tag() {
    // If the document somehow contains a literal `<html>` later (e.g. a
    // tutorial about HTML), we only rewrite the first one — the actual
    // root element. Subsequent occurrences are content.
    let html = "<html><body><pre>&lt;html&gt; means HTML</pre><html>second</html></body></html>";
    let result = inject_shell_frame_class(html);
    // First `<html` gets the class, second `<html` (after the pre block) is untouched.
    let first_class_pos = result.find("moss-shell-frame").unwrap();
    let second_html_pos = result.rfind("<html").unwrap();
    assert!(
        first_class_pos < second_html_pos,
        "only the first <html> tag should receive the class"
    );
    let second_count = result.matches("moss-shell-frame").count();
    assert_eq!(second_count, 1, "should not double-inject");
}

/// Regression: a CJK code point (3 bytes in UTF-8) straddling the
/// 4096-byte scan window caused `inject_shell_frame_class` to panic
/// with "byte index 4096 is not a char boundary" when slicing
/// `&html[..scan_limit]`. The fix walks `scan_limit` back to the
/// nearest char boundary before slicing.
///
/// Reproducer pattern from the wild: a CJK article with a long `<title>`
/// (a full Chinese headline plus the site name) and enough head
/// content (meta tags, syndication links, etc.) to push a CJK char
/// across byte 4096.
#[test]
fn inject_shell_frame_class_does_not_panic_on_cjk_at_scan_boundary() {
    // Construct a head that places a 3-byte CJK char straddling
    // bytes 4095..4098. The `<html>` tag is at byte 0 so it WILL be
    // found and rewritten; the test only asserts no panic.
    let prefix = "<html lang=\"zh-hans\"><head><title>";
    // Pad with ASCII so the next CJK char lands exactly across 4095.
    let pad_bytes = 4095 - prefix.len();
    let pad: String = "x".repeat(pad_bytes);
    let mut html = String::with_capacity(8192);
    html.push_str(prefix);
    html.push_str(&pad);
    html.push('光'); // 3 bytes: 4095..4098
    html.push_str("</title></head><body>x</body></html>");
    // Confirm we placed the boundary trap as intended.
    assert!(!html.is_char_boundary(4096), "test setup invariant");
    // Must not panic.
    let result = inject_shell_frame_class(&html);
    assert!(result.contains("moss-shell-frame"));
}

#[test]
fn shell_frame_class_only_injected_when_marker_present() {
    // Integration assertion: when the marker is absent, served HTML
    // must NOT carry `moss-shell-frame`. The bridge script's topology
    // check is then responsible for adding it at runtime if the doc
    // happens to be shell-mounted (defense-in-depth fallback).
    // When the marker IS present, the served HTML carries the class
    // server-side so first paint is correct.
    let baseline = "<html><body><h1>x</h1></body></html>";
    // No marker: HTML is unchanged by inject_shell_frame_class (we
    // only call this function when the marker is present, but verify
    // the chain by re-running through the marker check).
    assert!(!has_shell_marker(Some("foo=bar")));
    // With marker: rewrite happens.
    assert!(has_shell_marker(Some("__moss_shell=1")));
    assert!(inject_shell_frame_class(baseline).contains("moss-shell-frame"));
}

// ====================================================================
// Fix 1: server-side comment form URL rewrite + hint injection tests
// ====================================================================

#[test]
fn rewrite_comment_form_replaces_production_url() {
    // (a) Form with a production URL → rewritten to /__moss/comments.
    let html = r#"<html><body><form class="comment-form" id="moss-comment-form" data-server-url="https://api.mosspub.com/comments" data-site-name="x" data-page-key="/" data-page-title="T"></form></body></html>"#;
    let result = rewrite_comment_form_server_url(html);
    assert!(
        result.contains("data-server-url=\"/__moss/comments\""),
        "production URL must be rewritten to stub; got: {result}"
    );
    assert!(
        !result.contains("https://api.mosspub.com/comments"),
        "production URL must not remain after rewrite; got: {result}"
    );
}

#[test]
fn rewrite_comment_form_is_idempotent() {
    // (b) Idempotent: running rewrite twice yields the same result.
    let html = r#"<html><body><form id="moss-comment-form" data-server-url="https://api.mosspub.com/comments"></form></body></html>"#;
    let once = rewrite_comment_form_server_url(html);
    let twice = rewrite_comment_form_server_url(&once);
    assert_eq!(once, twice, "rewrite must be idempotent");
}

#[test]
fn rewrite_comment_form_unchanged_when_no_form() {
    // (c) HTML without the form → unchanged.
    let html = "<html><body><p>no form here</p></body></html>";
    let result = rewrite_comment_form_server_url(html);
    assert_eq!(result, html, "HTML without comment form must be unchanged");
}

#[test]
fn rewrite_comment_form_rewrites_empty_server_url() {
    // Inactive form (empty server_url, moss-service-inactive class on section).
    // Rewrite is safe and harmless: inactive state is determined by the section
    // class, not the attribute (verified in comment.rs render_comment_section).
    let html = r#"<section class="moss-service-inactive"><form id="moss-comment-form" data-server-url=""></form></section>"#;
    let result = rewrite_comment_form_server_url(html);
    assert!(
        result.contains("data-server-url=\"/__moss/comments\""),
        "inactive form (empty URL) must also be rewritten; got: {result}"
    );
    // The inactive class on the section is untouched.
    assert!(
        result.contains("moss-service-inactive"),
        "inactive section class must be preserved; got: {result}"
    );
}

#[test]
fn middleware_output_contains_stub_url_and_shim_not_production() {
    // (d) Integration: a page with a comment form, after the URL rewrite,
    // contains the stub URL and the shim script, and NOT the production URL.
    let html = r#"<!DOCTYPE html><html lang="en"><head></head><body>
<div class="comment-form-slot" id="default-form-slot"><form id="moss-comment-form" data-server-url="https://api.mosspub.com/comments"></form></div>
</body></html>"#;
    // Apply the URL rewrite (as the middleware does).
    let html = rewrite_comment_form_server_url(html);
    // Find </body> insertion point (mirrors middleware logic).
    let pos = html.rfind("</body>").unwrap();
    let shim_tag = format!(
            "<script type=\"text/javascript\" id=\"moss-comment-preview-shim\" data-moss-permanent>\n{}\n</script>\n",
            COMMENT_PREVIEW_SHIM
        );
    let result = format!("{}{}{}", &html[..pos], shim_tag, &html[pos..]);

    assert!(
        result.contains("data-server-url=\"/__moss/comments\""),
        "middleware output must carry stub URL; got fragment: {}",
        &result[..result.len().min(500)]
    );
    assert!(
        result.contains("id=\"moss-comment-preview-shim\""),
        "middleware output must contain shim script tag"
    );
    assert!(
        !result.contains("https://api.mosspub.com/comments"),
        "middleware output must NOT contain production URL"
    );
}

// ====================================================================
// B3: cross-file seam test — rewrite + hint track the real emitter
// ====================================================================
//
// Hand-written HTML fixtures in the tests above can silently drift if
// render_comment_section reorders attributes or changes marker strings.
// This test feeds the REAL emitter's output through both serve-time
// transforms so any such drift breaks immediately at compile/test time.

#[test]
fn strip_preview_only_scripts_removes_marked_regions() {
    let html = r#"<head><!--moss:no-preview--><script src="https://x.goatcounter.com/count.js"></script><!--/moss:no-preview--><title>t</title></head>"#;
    let out = strip_preview_only_scripts(html);
    assert!(
        !out.contains("goatcounter"),
        "marked analytics must be stripped in preview"
    );
    assert!(
        !out.contains("moss:no-preview"),
        "marker comments must be removed too"
    );
    assert!(
        out.contains("<title>t</title>"),
        "unmarked content must survive"
    );
}

#[test]
fn strip_preview_only_scripts_is_noop_without_markers() {
    let html = r#"<head><title>t</title></head>"#;
    assert_eq!(strip_preview_only_scripts(html), html);
}

// ====================================================================
// data-moss-preview guarantee — every served HTML page carries the gate
// ====================================================================
//
// The zero-flicker mechanism serves the previous frozen generation
// during rebuilds (and on cold start), and ship_phase strips
// data-moss-preview from frozen pages. Every runtime preview gate
// (subscribe.ts, the beacon's defense-in-depth check) is blind on such
// pages — e.g. a subscribe submit in that window would POST to
// production seta. The serve-time middleware must re-guarantee the
// attribute on every HTML response, generation-independent.

#[test]
fn ensure_preview_body_attr_adds_attribute_when_missing() {
    let html = "<html><head></head><body class=\"x\"><p>hi</p></body></html>";
    let out = ensure_preview_body_attr(html);
    assert!(
        out.contains("<body data-moss-preview class=\"x\">"),
        "ship-stripped page must get the attribute back, got: {out}"
    );
}

#[test]
fn ensure_preview_body_attr_adds_attribute_to_bare_body() {
    let html = "<html><body></body></html>";
    let out = ensure_preview_body_attr(html);
    assert!(out.contains("<body data-moss-preview>"), "got: {out}");
}

#[test]
fn ensure_preview_body_attr_noop_when_present() {
    let html = "<html><body data-moss-preview><p>hi</p></body></html>";
    assert_eq!(ensure_preview_body_attr(html), html);
}

#[test]
fn ensure_preview_body_attr_noop_without_body_tag() {
    let html = "<div>fragment</div>";
    assert_eq!(ensure_preview_body_attr(html), html);
}

#[test]
fn ensure_preview_body_attr_skips_body_literal_in_head_json_ld() {
    // Adversarial-review finding: escape_json_string does NOT escape
    // angle brackets, so an article titled `Styling the <body> element`
    // puts a literal `<body>` into head JSON-LD. First-substring-wins
    // would inject into that JSON string (corrupting it) and leave the
    // REAL body ungated — re-opening the exact hole this helper closes.
    let html = concat!(
        "<html><head>",
        r#"<script type="application/ld+json">{"headline":"Styling the <body> element in CSS"}</script>"#,
        "</head><body class=\"page\"><p>hi</p></body></html>",
    );
    let out = ensure_preview_body_attr(html);
    assert!(
        out.contains("<body data-moss-preview class=\"page\">"),
        "the REAL body must receive the attribute, got: {out}"
    );
    assert!(
        out.contains(r#"{"headline":"Styling the <body> element in CSS"}"#),
        "head JSON-LD must not be corrupted, got: {out}"
    );
}

#[test]
fn ensure_preview_body_attr_skips_lookalike_tags() {
    // <bodyguard> must not be treated as the body tag.
    let html = "<html><bodyguard x=\"1\"></bodyguard><body><p>hi</p></body></html>";
    let out = ensure_preview_body_attr(html);
    assert!(
        out.contains("<bodyguard x=\"1\">"),
        "lookalike untouched, got: {out}"
    );
    assert!(
        out.contains("<body data-moss-preview>"),
        "real body tagged, got: {out}"
    );
}

#[test]
fn strip_removes_the_real_beacon_emitter_output() {
    // Feed the REAL beacon emitter's output through the serve-time strip.
    // The zero-flicker window serves ship-stripped frozen generations
    // where the beacon's data-moss-preview self-gate is blind — this
    // serve-time strip is the primary preview protection, so the emitted
    // marker strings and the strip's marker constants must never drift.
    let deploy = crate::config::deployment::DomainDeploymentConfig {
        deploy_method: Some("moss".into()),
        site_id: Some("seam-test".into()),
        ..Default::default()
    };
    let slots = crate::build::features::generate_native_slots(
        &crate::config::services::ServicesConfig::default(),
        "/nonexistent-seam-test",
        false, // the email channel is not installed for this fixture
        crate::build::features::comment::MATTERS_DOMAIN_FALLBACK,
        &std::collections::HashMap::new(),
        &[],
        "en",
        None,
        Some(deploy),
        false,
        None,
        None, // no site-wide [site] comments preference in this fixture
    );
    let head_end = slots.get_html("head-end", "index.html").unwrap_or_default();
    assert!(
        head_end.contains("moss-beacon"),
        "precondition: emitter injects the beacon, got: {head_end}"
    );
    let out = strip_preview_only_scripts(&head_end);
    assert!(
        !out.contains("moss-beacon") && !out.contains("/beacon"),
        "served preview HTML must not carry the beacon, got: {out}"
    );
}

#[test]
fn rewrite_tracks_the_real_emitter() {
    // The FORM_MARKER substring must byte-match what render_comment_section
    // emits. Hand-written fixtures can't catch an attribute reorder in
    // render.rs — this test feeds the real emitter's output through the
    // serve-time URL rewrite.
    let html = crate::build::features::comment::render_comment_section(
        &[],
        "https://api.mosspub.com/comments",
        "example.com",
        "abc12345",
        "Test",
        crate::i18n::Language::En,
        &[],
        0,
    );
    let page = format!("<html lang=\"en\"><body>{html}</body></html>");
    let rewritten = rewrite_comment_form_server_url(&page);
    assert!(
        rewritten.contains("data-server-url=\"/__moss/comments\""),
        "rewrite must track the emitter; got fragment: {}",
        &rewritten[..rewritten.len().min(1000)]
    );
    assert!(
        !rewritten.contains("https://api.mosspub.com/comments"),
        "production URL must be absent after rewrite"
    );
}

#[test]
fn detect_page_lang_buckets() {
    assert_eq!(detect_page_lang(r#"<html lang="en">"#), PreviewLang::En);
    assert_eq!(
        detect_page_lang(r#"<html lang="zh-hans">"#),
        PreviewLang::ZhHans
    );
    assert_eq!(
        detect_page_lang(r#"<html lang="zh-CN">"#),
        PreviewLang::ZhHans
    );
    assert_eq!(
        detect_page_lang(r#"<html lang="zh-hant">"#),
        PreviewLang::ZhHant
    );
    assert_eq!(
        detect_page_lang(r#"<html lang="zh-TW">"#),
        PreviewLang::ZhHant
    );
    assert_eq!(
        detect_page_lang(r#"<html lang="zh-Hant">"#),
        PreviewLang::ZhHant
    );
    assert_eq!(
        detect_page_lang(r#"<html lang='zh-hans'>"#),
        PreviewLang::ZhHans
    );
    assert_eq!(detect_page_lang(r#"<html>"#), PreviewLang::En);
    // A later "tw"/"hant" substring elsewhere in the doc must not leak in.
    assert_eq!(
        detect_page_lang(r#"<html lang="en"><a>twitter hant</a>"#),
        PreviewLang::En
    );
}

#[test]
fn tooltip_style_carries_lang_correct_copy() {
    let zh = build_preview_tooltip_style(PreviewLang::ZhHans);
    assert!(
        zh.contains("本地预览·评论不会发送到服务器"),
        "comment zh-hans copy: {zh}"
    );
    assert!(
        zh.contains("本地预览·订阅不会发送到服务器"),
        "subscribe zh-hans copy: {zh}"
    );
    let zht = build_preview_tooltip_style(PreviewLang::ZhHant);
    assert!(
        zht.contains("本地預覽·評論不會發送到伺服器"),
        "comment zh-hant copy: {zht}"
    );
    assert!(
        zht.contains("本地預覽·訂閱不會發送到伺服器"),
        "subscribe zh-hant copy: {zht}"
    );
    let en = build_preview_tooltip_style(PreviewLang::En);
    assert!(
        en.contains("Local preview · comment won't be sent"),
        "comment en copy: {en}"
    );
}

#[test]
fn tooltip_style_targets_both_buttons_and_no_inline_hint() {
    let css = build_preview_tooltip_style(PreviewLang::En);
    assert!(
        css.contains(".comment-form-submit:hover::after"),
        "comment selector: {css}"
    );
    assert!(
        css.contains(".moss-btn-slot:hover::after"),
        "subscribe selector: {css}"
    );
    // The visible inline hint is gone — no div, no bare moss-btn targeting.
    assert!(
        !css.contains("comment-preview-hint"),
        "must not emit the old hint div"
    );
    assert!(
        !css.contains(".moss-btn:hover"),
        "must not target the generic .moss-btn"
    );
    assert!(
        css.contains(".comment-form-slot{overflow:visible}"),
        "comment-form-slot overflow override: {css}"
    );
}

#[test]
fn owner_controls_injected_only_with_shell_marker() {
    let html = "<html><body><ol><li class=\"comment-item\" \
            data-comment-source=\"artalk\" data-comment-id=\"1\"></li></ol></body></html>";

    let with_marker = inject_preview_assets(html, true);
    assert!(
        with_marker.contains("moss-comment-owner-controls"),
        "owner-controls script must be injected under the shell marker"
    );
    assert!(
        with_marker.contains("data-moss-hide-comment"),
        "owner-controls style must reference the hide button"
    );

    let without_marker = inject_preview_assets(html, false);
    assert!(
        !without_marker.contains("moss-comment-owner-controls"),
        "owner-controls must be ABSENT without the shell marker (never shipped)"
    );
    assert!(
        without_marker.contains("moss-bridge"),
        "bridge must inject even without shell marker"
    );
    assert!(
        without_marker.contains("moss-comment-preview-shim"),
        "shim must inject even without shell marker"
    );
}

// ---- Blueprint placeholder (preview-only, injected into <head>) ----

/// The placeholder goes into `<head>`, not before `</body>` like every other
/// preview injection.
///
/// It registers a capture-phase `error` listener. An image can fail while the
/// parser is still working through the body, so a body-end script would miss it
/// — and a missed error is the browser's broken-image icon on a reader-facing
/// preview.
#[test]
fn the_placeholder_is_injected_first_inside_head() {
    let html = "<html><head><title>x</title></head><body><img src=\"/a.webp\"></body></html>";
    let out = inject_placeholder_into_head(html);

    let script = out.find("moss-img-fallback").expect("placeholder must inject");
    let head_end = out.find("</head>").expect("head must survive");
    let title = out.find("<title>").expect("title must survive");
    assert!(script < head_end, "the placeholder must be inside <head>");
    assert!(script < title, "it must precede everything else in <head>");
    assert!(
        out.contains("addEventListener(\"error\"") || out.contains("addEventListener('error'"),
        "the injected bundle must carry the error listener"
    );
}

/// The marker outline rides along, because `site.css` no longer carries it —
/// styling a class that only exists in the preview would ship dead bytes to
/// every published site.
#[test]
fn the_placeholder_brings_its_own_marker_style() {
    let out = inject_placeholder_into_head("<html><head></head><body></body></html>");
    assert!(out.contains("moss-img-fallback-style"));
    assert!(out.contains("outline"));
}

/// A `<head>` with attributes is still a `<head>`. The insert point is the end
/// of the open tag, not a literal `<head>` match.
#[test]
fn an_attributed_head_still_gets_the_placeholder() {
    let out = inject_placeholder_into_head("<html><head data-x=\"1\"><meta></head><body></body></html>");
    let script = out.find("moss-img-fallback").expect("placeholder must inject");
    assert!(script < out.find("<meta>").unwrap(), "inserted after the open tag, before its children");
}

/// Malformed input is passed through rather than corrupted — same contract as
/// `inject_preview_assets` with no `</body>`.
#[test]
fn a_document_with_no_head_is_left_alone() {
    let html = "<div>fragment</div>";
    assert_eq!(inject_placeholder_into_head(html), html);
}
