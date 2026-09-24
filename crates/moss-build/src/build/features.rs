//! Native site features (review, comment, email, analytics).
//!
//! These features were previously JS plugins and are now built into the
//! Rust build pipeline for speed and tighter integration.
//!
//! The core function is `generate_native_slots()` which produces `ResolvedSlots`
//! that get merged with any remaining plugin slots and injected into staged HTML
//! before ship, manifest seal, hash persistence, and preview diffing.

pub mod comment;
pub mod email;
pub mod review;
pub mod sync;

/// Get the inactive tooltip text for the given language.
pub(crate) fn inactive_label(lang: crate::i18n::Language) -> &'static str {
    crate::i18n::t(lang, "available_after_publishing")
}

/// Shared check-mark SVG used inside `.moss-btn__check` spans for both the
/// subscribe and comment submit buttons. Extracted here so both emitters stay
/// byte-for-byte identical (no-duplication rule).
pub(crate) const CHECK_SVG: &str = r##"<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M5 12.5l4.5 4.5L19 7" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"/></svg>"##;

/// Basic HTML escaping for attribute values and text content.
pub(crate) fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

use crate::build::slots::Slot;
use crate::build::render::resolve_comments_pref;
use crate::config::services::ServicesConfig;
use crate::config::deployment::DomainDeploymentConfig;
use crate::build::enhance::{EnhanceContent, EnhanceResult, ResolvedSlots};
use std::collections::HashMap;

/// Decide whether to inject the moss-hosted subscribe JS+CSS bundle.
///
/// Returns `true` if EITHER:
/// - the email channel is configured (`[channels.email]`), OR
/// - any page in the build set uses an inline `:::subscribe` shortcode.
///
/// The bundle de-dupes via the slots-merge key, so this returning `true`
/// twice in one build still embeds the script exactly once.
pub fn should_inject_subscribe_assets(
    pages: &[crate::build::types::ParsedDocument],
    email_installed: bool,
) -> bool {
    email_installed
        || pages.iter().any(|p| p.features.inline_subscribe)
        || pages.iter().any(|p| p.features.inline_apply)
}

// `project_has_inline_subscribe(folder_path)` was removed in PR7b.
//
// It was a temporary filesystem-scan stand-in for "does any page in this
// build use the `:::subscribe` shortcode?", reading every .md file under
// the project root looking for a `:::subscribe` substring. The proper
// answer is on the parsed page: `ParsedDocument.features.inline_subscribe`
// is populated at parse time by `process_markdown_file`. The call site
// in `build.rs::run_pipeline` now reads
// `pages.iter().any(|p| p.features.inline_subscribe)` directly.
//
// `should_inject_subscribe_assets` below already consumes that flag.

/// Generate native feature slots from config and social data.
///
/// This produces ResolvedSlots that can be merged with plugin slots and
/// injected into staged HTML before final build artifacts are sealed.
///
/// `cached_deploy_config` — if provided, reuses an already-loaded domain
/// config instead of re-reading .moss/state.toml from disk.
///
/// `pages` — the parsed documents in the build set, used to detect features
/// like inline `:::subscribe` shortcodes that trigger asset injection
/// independently of the email-channel gate.
///
/// `start_server` — mirrors `PipelineConfig.start_server`: `true` when this
/// build will boot the embedded preview server after it completes.
/// Orchestration only — build output is mode-independent by design, so
/// never read this as "is this a preview build".
///
/// Analytics and beacon are both mode-INDEPENDENT: injected in every
/// build mode so the published artifact is deterministic regardless of
/// whether the build was a folder-open (start_server=true) or a watch
/// rebuild (start_server=false). Preview safety is handled at the
/// runtime layer, not by omitting the tags:
///
/// - **Analytics pixel** — wrapped in `<!--moss:no-preview-->` markers;
///   the preview server strips those regions before serving
///   (`preview::iframe_bridge::strip_preview_only_scripts`). The
///   deployed artifact keeps the script; the markers are removed by
///   `apply_transform` on ship (Task A5).
/// - **Pageview beacon** — two cooperating preview protections:
///   (1) wrapped in the same `<!--moss:no-preview-->` markers, so
///   preview-served STAGING pages drop the script entirely at serve
///   time. `ship_phase` removes only the marker comments, so deployed
///   pages keep the script — which means frozen generations served
///   during the zero-flicker window still carry it (their markers are
///   already gone) and the strip cannot help there.
///   (2) the runtime `DOMContentLoaded` self-gate on `data-moss-preview`
///   (Task A4) — effective on EVERY preview-served page because the
///   preview server re-guarantees the attribute on each HTML response
///   (`preview::iframe_bridge::ensure_preview_body_attr`), including
///   ship-stripped frozen generations where the build-time annotation
///   is gone. On the live site neither mechanism applies and the
///   beacon fires.
///
/// Comments (Artalk) are mode-INDEPENDENT: baked identically in every
/// mode (preview, build, publish). Preview safety is achieved by two
/// cooperating mechanisms in `preview::iframe_bridge`:
/// (1) a serve-time rewrite that replaces the form's `data-server-url`
/// with `/__moss/comments` on every served HTML response, and
/// (2) the comment-preview-shim.ts injected before `</body>` as
/// defense-in-depth. No gate applies to comments any longer.
///
/// Subscribe forms (the seta footer form and inline `:::subscribe`) are
/// mode-INDEPENDENT: identical markup in every build mode, with preview
/// safety handled at runtime by subscribe.ts's `data-moss-preview` gate
/// (pinned by `email_slots_identical_across_build_modes`).
///
/// Note: the gate is narrower than "any dev-side build" — `moss build
/// --watch` runs with `start_server: false` and still emits both.
///
/// `media_lookup` — when `Some`, the review colophon's cover image routes
/// through `image_render::synthesize_image_html` (the single-emission path
/// shared by every `<img>` in moss output). This gates `<source
/// srcset="X.webp">` emission on manifest presence — eliminating the
/// WebP-404 failure mode for the colophon — and adds attribute-injection
/// (`width`/`height`/`loading`/`data-placeholder-src`) at the typed-data
/// layer. `None` is the test/fragment-rendering fallback.
pub fn generate_native_slots(
    services: &ServicesConfig,
    project_path: &str,
    // Whether the `email` channel is installed for this project. Handed in
    // rather than read here: answering it means parsing `.moss/config.toml`'s
    // `[channels]` table through `plugins::discovery`, which the compiler stays
    // clear of outright — so the caller, which
    // already knows the answer, says it.
    email_installed: bool,
    // The Matters domain for comment attribution, resolved by the caller for
    // the same reason as `email_installed` — see
    // `comment::load_all_social_comments`.
    matters_domain: &str,
    article_map: &HashMap<String, ArticleInfo>,
    pages: &[crate::build::types::ParsedDocument],
    lang: &str,
    domain: Option<&str>,
    cached_deploy_config: Option<DomainDeploymentConfig>,
    start_server: bool,
    media_lookup: Option<&crate::build::media::dimensions::MediaDimensionLookup>,
    // The site-wide `[site] comments` default, resolved by the caller from
    // the SAME `.moss/config.toml` parse `pipeline.rs::build_inner` already
    // did to build `render::SiteConfig` (`site_bool("comments")` at
    // pipeline.rs ~1283) — handed in rather than re-read here for the same
    // H-config reason as `email_installed`: a second parse of
    // the same file is a second ladder that can drift from the first, not a
    // saved read (H-config already made this ONE read for the whole build).
    site_comments: Option<bool>,
) -> ResolvedSlots {
    // Convert the raw site-config tag to a typed Language ONCE at this
    // boundary; everything below matches on `Language` (zh-hant safe).
    // This is the SITE-DEFAULT language — used only as the fallback for pages
    // that aren't in `pages` (e.g. synthesized listing pages) and for genuinely
    // site-level output.
    let site_lang = crate::i18n::Language::from_bcp47_lenient(lang);

    // Per-page resolved language. Localized per-page output (the comment
    // section, the auto-injected subscribe form) must follow the HOSTING
    // page's OWN language (doc.lang), not the site default — otherwise an
    // English page in a Chinese-majority site renders Chinese comment/form
    // copy. `pages` carries the fully-resolved `ParsedDocument.lang`
    // (frontmatter > filename suffix > folder inheritance > content detection
    // > site default), so it is the authoritative source; `site_lang` is only
    // the fallback for pages absent from `pages`. Keyed by uid (the
    // article_map / comment page_key) with a url_path fallback for entries
    // whose uid is unset.
    let page_lang_by_uid: std::collections::HashMap<&str, crate::i18n::Language> = pages
        .iter()
        .filter_map(|p| p.uid.as_deref().map(|u| (u, p.lang)))
        .collect();
    let page_lang_by_url: std::collections::HashMap<&str, crate::i18n::Language> =
        pages.iter().map(|p| (p.url_path.as_str(), p.lang)).collect();

    let mut slots = ResolvedSlots::empty();
    let deploy_config = cached_deploy_config
        .unwrap_or_else(|| crate::build::site_config::get_domain_config(project_path).unwrap_or_default());

    // Resolve the hosting environment once for the entire slot-generation pass.
    // Used by: subscribe form action URL and the comment server URL.
    // Mirrors the pattern used by the analytics beacon (line ~366).
    let env = crate::build::site_config::resolve_environment(project_path);
    let seta_url = env.seta_url();

    // Derive supported_scopes via the single source of truth shared with the
    // email settings command (email::derive_language_sections): a top-level
    // folder is a scope when its NAME is a language code OR its home shares the
    // root home's translationKey. /about/ is neither, so it is not a scope. The
    // picker reads the build-persisted result of this SAME judge, so the
    // subscribe form and the modal can never disagree.
    let supported_scopes: Vec<String> = email::site_scopes(pages);

    // Analytics → head-end (static, all pages). Mode-INDEPENDENT: injected in
    // every build mode so the published artifact is deterministic. Preview
    // safety is the serve-time strip (preview server), since a foreign
    // analytics script cannot self-gate. (See deploy-generation design §6.)
    if let Some(analytics_script) = services
        .analytics
        .as_ref()
        .and_then(|a| a.script.as_deref())
    {
        use crate::build::markdown::AnalyticsConfig as FrontmatterAnalytics;
        let ac = FrontmatterAnalytics {
            url: analytics_script.to_string(),
            provider: None,
            site_id: None,
        };
        // Wrap in a preview-strip marker so the preview server can remove it
        // from served responses (Task A5) while the deployed artifact keeps it.
        let script_tag = format!(
            "<!--moss:no-preview-->{}<!--/moss:no-preview-->",
            ac.to_script_tag()
        );
        let result = EnhanceResult {
            success: true,
            slots: HashMap::from([(Slot::HeadEnd.as_str().into(), EnhanceContent::Static { html: script_tag })]),
        };
        slots.merge(&result, 1, "__native_analytics");
    }

    // Review CSS + colophon → head-end + after-title (per-page)
    let review_data = review::load_review_data(project_path);
    if let Some(ref data) = review_data {
        let mut per_page: HashMap<String, String> = HashMap::new();
        for (uid, info) in article_map {
            if let Some(ref review_of) = info.review_of {
                // Find review by source_url matching review_of, or by uid
                let review_item = data
                    .articles
                    .get(uid.as_str())
                    .or_else(|| {
                        data.articles
                            .values()
                            .find(|r| r.source_url == *review_of)
                    });

                if let Some(item) = review_item {
                    let colophon = review::render_colophon(item, info.cover.as_deref(), media_lookup);
                    per_page.insert(info.url_path.clone(), colophon);
                }
            }
        }

        if !per_page.is_empty() {
            // CSS (static, all pages) — a <link> to a content-hashed file,
            // not an inline <style>. This slot is site-wide, so an inline
            // block is replicated into every page and re-downloaded on every
            // navigation; see `build::emit::feature_styles`. Still
            // `@layer plugins`, so user `.moss/theme/style.css` (in
            // @layer themes) overrides `.moss-review-*` without specificity
            // fights — the wrapping moved into the emitted file.
            let css_result = EnhanceResult {
                success: true,
                slots: HashMap::from([(
                    Slot::HeadEnd.as_str().into(),
                    EnhanceContent::Static { html: slots.link_feature_style("review") },
                )]),
            };
            slots.merge(&css_result, 2, "__native_review");

            // Per-page colophon
            let colophon_result = EnhanceResult {
                success: true,
                slots: HashMap::from([(
                    "after-title".into(),
                    EnhanceContent::PerPage { pages: per_page },
                )]),
            };
            slots.merge(&colophon_result, 2, "__native_review_content");
        }
    }

    // Comments CSS + section → head-end + after-article (per-page)
    // Load ALL social sources (comment.json, matters.json, douban.json, etc.)
    // All keyed by uid.
    //
    // Honor explicit `enabled = false` by filtering the Option itself: a
    // present-but-disabled section behaves like an absent section (comments
    // are opt-in on every host — see ServicesConfig::is_enabled).
    // Found while dogfooding a real site, 2026-04-30.
    //
    // Design §7: the baked artifact is mode-independent. The preview shim
    // (injected at serve time by iframe_bridge.rs, never written to disk)
    // handles the preview-specific behavior — there is no reason to skip
    // comment rendering here based on start_server.
    if let Some(ref comments_config) = services.comments
        .as_ref()
        .filter(|c| c.common.enabled != Some(false))
    {
        // Self-hosted override wins; otherwise the moss-operated server derives
        // from the environment (config can never strand a site on a dead host
        // again — see migration v4). Undeployed sites render the inactive form.
        // Same helper as the sync gate (CommentsService::resolved_server_url) — resolution order written once.
        let resolved_server_url =
            comments_config.resolved_server_url(domain, deploy_config.site_id.as_deref(), &env);
        let mut all_comments = comment::load_all_social_comments(project_path, matters_domain);

        // Compute the per-page high-water Artalk comment id BEFORE moderation removes
        // any hidden comments. This is the max id the build knows about for each page
        // from the FULL synced set. The client uses it to gate which fetched Artalk
        // comments are appended on stale-while-revalidate: a comment with id ≤ high-water
        // was known at bake time (including hidden ones) and must never be re-appended;
        // only a comment with id > high-water is genuinely new (posted after the bake).
        // Artalk comment ids are monotonic integers, so this is timezone-skew-safe.
        let artalk_high_water: HashMap<String, i64> = all_comments
            .iter()
            .map(|(uid, comments)| {
                let max_id = comments
                    .iter()
                    .filter(|c| c.source == "artalk")
                    .filter_map(|c| c.id.parse::<i64>().ok())
                    .max()
                    .unwrap_or(0);
                (uid.clone(), max_id)
            })
            .collect();

        // Apply owner moderation: signed hide/unhide events (moderation.jsonl) decide
        // visibility, replacing the legacy replica-tombstone path. No events → no-op
        // (the identity is not even loaded).
        let mod_events = comment::moderation::load_mod_events(project_path);
        if !mod_events.is_empty() {
            let mut id_svc =
                crate::identity::service::IdentityService::new(std::path::Path::new(project_path));
            if let Some(pubkey) = id_svc.get().ok().flatten().map(|i| i.pubkey.clone()) {
                for comments in all_comments.values_mut() {
                    comment::reduce::retain_visible(comments, &mod_events, &pubkey);
                }
            }
        }
        // site_name must match the artalk site identifier (deploy_config.site_id),
        // not the custom domain — artalk sites are keyed by site_id, not hostname.
        let site_name = deploy_config.site_id.as_deref().unwrap_or("localhost");

        // Who can hold a conversation? Every page, not just the articles.
        //
        // The article map deliberately omits folder-index pages: they are not
        // syndicatable and plugins must not see them as articles (the reasoning
        // is written out in `build_article_map`). Comments are a different
        // question, and the answer does not follow from the page having
        // children — so an author who turns comments on for a folder page (or
        // for the homepage, which is a folder index too) gets them.
        // Folder pages carry no `syndicated:` link-outs, since nothing can
        // syndicate them.
        let folder_targets: Vec<(String, ArticleInfo)> = pages
            .iter()
            .filter(|p| !p.slot_only && p.kind == moss_core::PageKind::Folder)
            .filter_map(|p| {
                let uid = p.uid.clone()?;
                Some((uid.clone(), ArticleInfo {
                    // Raw url_path, not the pretty form the article map stores:
                    // `to_pretty_url("index.html")` is the empty string, which no
                    // page path can ever match, so the homepage would drop out.
                    // `get_html` tries the pretty key first and the raw one second.
                    url_path: p.url_path.clone(),
                    title: p.title.clone(),
                    uid,
                    review_of: None,
                    rating: None,
                    cover: None,
                    comments: p.comments,
                    lang: None,
                    syndicated: Vec::new(),
                }))
            })
            .collect();

        let mut per_page: HashMap<String, String> = HashMap::new();
        // `[site] comments` sets the ARTICLE default only: an author can dial
        // comments off site-wide without touching every article's
        // frontmatter. Folder pages stay opt-in-only regardless of the
        // site-wide value (`site_default: None` below) — a site owner
        // turning comments ON site-wide should not suddenly grow a comment
        // section under every listing page, only under articles. `page_value`
        // (an explicit per-page `comments:`) always wins over either default,
        // via the same `resolve_comments_pref` ladder `resolve_comments_attr`
        // uses for the `data-comments` JS hint — one resolution, not two that
        // can drift (that drift is how `[site] comments = false` used to
        // leave the section rendered while only the JS attribute went false).
        //
        // Articles default ON (`comments: false` opts out); folder pages —
        // the homepage among them — default OFF (`comments: true` opts in).
        // A fix that let folder pages show comments at all
        // chained them onto the article loop with the article default, so an
        // author who never touched `comments:` got a section on every folder
        // page including the homepage. The homepage having no explicit
        // frontmatter is the common case, not the exception, so that default
        // put a comment button under every generated site's landing page.
        for (uid, info, default_on, site_default) in article_map
            .iter()
            .map(|(u, i)| (u, i, true, site_comments))
            .chain(folder_targets.iter().map(|(u, i)| (u, i, false, None)))
        {
            let show = resolve_comments_pref(info.comments, site_default).unwrap_or(default_on);
            if !show {
                continue;
            }

            let empty_vec = Vec::new();
            let comments = all_comments.get(uid.as_str()).unwrap_or(&empty_vec);
            let high_water_id = artalk_high_water.get(uid.as_str()).copied().unwrap_or(0);

            // uid is the canonical comment page_key; legacy pathname keys
            // are only normalized at sync/import time.
            //
            // Localize the comment UI in the HOSTING page's own language, not
            // the site default: an English page in a Chinese-majority site
            // must show English comment copy. Resolve from `pages` (uid, then
            // url_path), falling back to the site default for pages absent
            // from `pages`.
            let page_lang = page_lang_by_uid
                .get(uid.as_str())
                .or_else(|| page_lang_by_url.get(info.url_path.as_str()))
                .copied()
                .unwrap_or(site_lang);
            let section_html = comment::render_comment_section(
                comments,
                &resolved_server_url,
                site_name,
                uid,
                &info.title,
                page_lang,
                &info.syndicated,
                high_water_id,
            );
            per_page.insert(info.url_path.clone(), section_html);
        }

        if !per_page.is_empty() {
            // CSS — the largest of the three at 18 KB, and the clearest case
            // for a <link>: inline it was 18 KB on every page of the site,
            // uncached. Still `@layer plugins` (inside the emitted file) so
            // user `.moss/theme/style.css` overrides `.comment-*` without
            // specificity fights.
            let css_result = EnhanceResult {
                success: true,
                slots: HashMap::from([(
                    Slot::HeadEnd.as_str().into(),
                    EnhanceContent::Static { html: slots.link_feature_style("comments") },
                )]),
            };
            slots.merge(&css_result, 3, "__native_comments");

            // Per-page sections
            let section_result = EnhanceResult {
                success: true,
                slots: HashMap::from([(
                    "after-article".into(),
                    EnhanceContent::PerPage { pages: per_page },
                )]),
            };
            slots.merge(&section_result, 3, "__native_comments_content");
        }
    }

    // Beacon script → head-end (static, all pages)
    // Auto-injected for moss deployments, regardless of analytics config.
    // Sends a fire-and-forget pageview POST to the beacon endpoint.
    // Mode-INDEPENDENT: injected in every mode. Preview safety is layered:
    // - The <!--moss:no-preview--> wrapper: the preview server strips the
    //   region from served STAGING pages (same mechanism as the analytics
    //   pixel). ship_phase removes only the marker comments, so the deployed
    //   artifact keeps the script — as do frozen generations served during
    //   the zero-flicker window, whose markers are already gone.
    // - The data-moss-preview self-gate (Task A4) below covers those frozen
    //   pages: the preview server re-guarantees the attribute on every HTML
    //   response (iframe_bridge::ensure_preview_body_attr), so the gate
    //   holds even where the build-time annotation was ship-stripped.
    // - The loopback-origin gate below covers ship output served OUTSIDE the
    //   preview server (the documented `python3 -m http.server` flow,
    //   file:// opens): those pages are ship-stripped and un-middlewared, so
    //   neither mechanism above applies. Local-env deploys live on
    //   *.localhost subdomains and still pass.
    {
        if deploy_config.deploy_method.as_deref() == Some("moss") {
            if let Some(ref site_id) = deploy_config.site_id {
                let beacon_script = format!(
                    concat!(
                        "<!--moss:no-preview-->",
                        "<script id=\"moss-beacon\">(function(){{",
                        "document.addEventListener('DOMContentLoaded',function(){{",
                        "if(document.body.hasAttribute('data-moss-preview'))return;",
                        "var h=location.hostname;",
                        "if(location.protocol==='file:'||h==='localhost'||h==='127.0.0.1'||h==='[::1]')return;",
                        // Absolute URL with the environment's own scheme
                        // (https for prod/staging, http for local seta) —
                        // never protocol-relative: an http page origin would
                        // POST to http://, whose 308 https-upgrade redirect
                        // is fatal to the CORS preflight.
                        "var b='{}/api/sites/{}/beacon';",
                        "var u=new URLSearchParams(location.search);",
                        "var s=function(d){{fetch(b,{{",
                        "method:'POST',",
                        "headers:{{'Content-Type':'application/json'}},",
                        "body:JSON.stringify(d),",
                        "keepalive:true",
                        // Analytics is fire-and-forget: a failed ping must not
                        // surface as an unhandled rejection on readers' consoles.
                        "}}).catch(function(){{}})}};
",
                        "s({{",
                        "path:location.pathname,",
                        "referrer:document.referrer||null,",
                        "screen_bucket:innerWidth<768?'mobile':innerWidth<1024?'tablet':'desktop',",
                        "utm_source:u.get('utm_source'),",
                        "utm_medium:u.get('utm_medium')",
                        "}});
",
                        "document.querySelectorAll('[data-track]').forEach(function(el){{",
                        "el.addEventListener('click',function(){{",
                        "s({{path:location.pathname,event:el.getAttribute('data-track')}})",
                        "}})",
                        "}})",
                        "}});",
                        "}})()</script>",
                        "<!--/moss:no-preview-->"
                    ),
                    seta_url,
                    html_escape(site_id)
                );
                let result = EnhanceResult {
                    success: true,
                    slots: HashMap::from([(
                        Slot::HeadEnd.as_str().into(),
                        EnhanceContent::Static { html: beacon_script },
                    )]),
                };
                slots.merge(&result, 0, "__native_beacon");
            }
        }
    }

    // Subscribe CSS+JS and seta form: injected in both preview and build.
    // The form markup is identical in all modes; preview safety is the
    // runtime `data-moss-preview` gate in subscribe.ts (a submit under
    // preview runs the success animation locally instead of POSTing), so
    // the user sees the real layout but cannot create real subscribers.
    {
        // CSS+JS assets: always inject when email is in play so the inactive
        // form in preview is styled correctly.
        if should_inject_subscribe_assets(pages, email_installed) {
            // CSS as a <link> (see `build::emit::feature_styles`); the
            // subscribe script stays inline. The script is ~2 KB and its
            // absence breaks the form outright, so the extra round trip buys
            // less than it costs — the file/inline line is drawn on size and
            // site-wideness, not on principle.
            let css_result = EnhanceResult {
                success: true,
                slots: HashMap::from([(
                    Slot::HeadEnd.as_str().into(),
                    EnhanceContent::Static {
                        html: format!(
                            "{}<script class=\"moss-subscribe-script\">{}</script>",
                            slots.link_feature_style("email"),
                            email::SUBSCRIBE_JS
                        ),
                    },
                )]),
            };
            slots.merge(&css_result, 5, "__native_moss_subscribe_css");
        }

        // Seta subscribe form: injected in EVERY build mode. Wired when a
        // site_id exists; pending (action="#", data-moss-pending-site) when
        // the site hasn't been published yet, so preview shows the real
        // footer from day one.
        //
        // When footer.md is present moss renders it verbatim and the user
        // places the form via `:::subscribe`. When footer.md is absent, moss
        // emits a minimal default footer with the subscribe form.
        //
        // Slot::FooterEnd places the form AFTER the default link list — links
        // lead, the auto-injected widget trails.
        // High-signal diagnostic, gated on `email_installed` so off-by-default
        // sites stay quiet: the subscribe footer is the slot most likely to
        // "appear then disappear" across rebuilds, yet nothing on this path was
        // logged (ticket LOG-325B-T1605 was undiagnosable for exactly this
        // reason). Recording whether THIS build emitted the form lets Send-Logs
        // tell "emitted but refresh suppressed" apart from "never emitted
        // (channel config read stale)".
        // Stat for footer.md only when the email channel is in play (the
        // subscribe form is its only consumer) — keeps email-off rebuilds free,
        // preserving the original `email_installed && …` short-circuit.
        let has_footer_file =
            email_installed && crate::build::footer::project_has_footer_file(project_path);
        if email_installed {
            log::info!(
                "[subscribe] email channel installed → default footer form {}",
                if has_footer_file {
                    "SUPPRESSED (footer.md present; form placed via :::subscribe)"
                } else {
                    "EMITTED into FooterEnd slot"
                }
            );
        }
        if email_installed && !has_footer_file {
            // An empty site_id is not wiring: it would render a "wired"
            // form posting to /sites//subscribe. Treat it as absent
            // (the invariant the deleted get_email_policy_warnings
            // command used to defend).
            let site_id = deploy_config.site_id.as_deref().filter(|s| !s.is_empty());
            // The "default email footer" is just the :::subscribe shortcode
            // injected into footer-end: one renderer for both. `site_id` is
            // already filtered to non-empty, so None → pending form.
            // `form_lang` localizes the form copy in the hosting page's own
            // language (see the scoped branch below); the single-scope Static
            // form uses the site default, which for a single-scope (=single-
            // language) site IS the page language.
            let render_form = |scope: &str, form_lang: crate::i18n::Language| {
                email::render_hosted_subscribe_form(site_id, form_lang, scope, None, None, seta_url)
            };
            // Single-scope site (no language scopes): every page's scope is
            // "", so emit ONE Static form. Static covers every HTML file the
            // injector walks — including SYNTHESIZED pages (the auto-generated
            // homepage of a site without an index.md/self-named home note,
            // folder listing pages) that are absent from `pages` and would be
            // missed by a PerPage map keyed on parsed url_paths.
            //
            // Scoped sites keep per-page forms so each language scope
            // subscribes to its own newsletter. KNOWN GAP: their synthesized
            // pages get no form (not in `pages`) — see the design doc's
            // Follow-ups.
            let footer_end_content = if supported_scopes.is_empty() {
                EnhanceContent::Static { html: render_form("", site_lang) }
            } else {
                let mut per_page_forms: HashMap<String, String> = HashMap::new();
                for doc in pages {
                    let scope = email::detect_page_scope(&doc.url_path, &supported_scopes);
                    // Per-page form copy follows the hosting page's own
                    // language (doc.lang) — a scoped site is multilingual, so
                    // an English page must get an English form.
                    per_page_forms.insert(doc.url_path.clone(), render_form(&scope, doc.lang));
                }
                EnhanceContent::PerPage { pages: per_page_forms }
            };
            let form_result = EnhanceResult {
                success: true,
                slots: HashMap::from([
                    (Slot::FooterEnd.as_str().into(), footer_end_content),
                ]),
            };
            slots.merge(&form_result, 100, "__native_moss_subscribe_form_default");
        }
    }

    // Author-customized footer slots (footer.md at site root and per language
    // tree, plus any file with `slot: footer-left` in frontmatter).
    //
    // Bucketed by the source file's language tree: a `zh-hans/footer.md` is
    // injected on zh-hans pages, the root `footer.md` on everything else. When
    // a site has only the default (single-language) footer this collapses to a
    // `Static` entry — byte-identical to the pre-i18n behavior — and only emits
    // `PerLanguage` when language-specific footers actually exist.
    let mut footer_by_lang = crate::build::footer::collect_footer_slots_by_language(pages);
    // A footer/slot source this build could not read (most often still
    // downloading from the cloud) has no `ParsedDocument` to contribute above,
    // so without this the chrome it fills would vanish from every page on the
    // site rather than just staying stale on this one build. See
    // `apply_last_known_good_fallback` for the tradeoffs and `refuse_publish`
    // for the backstop that keeps a build built this way from shipping quietly.
    let stale_footer_buckets =
        crate::build::footer::apply_last_known_good_fallback(project_path, &mut footer_by_lang);
    if !stale_footer_buckets.is_empty() {
        log::warn!(
            "[footer] falling back to last-known-good content for {} — this build could not read it",
            stale_footer_buckets.join(", ")
        );
    }
    for (slot_name, footer) in &footer_by_lang {
        let content = if footer.by_lang.is_empty() {
            match &footer.default {
                Some(html) => EnhanceContent::Static { html: html.clone() },
                None => continue,
            }
        } else {
            EnhanceContent::PerLanguage {
                default: footer.default.clone(),
                by_lang: footer.by_lang.clone(),
            }
        };
        let result = EnhanceResult {
            success: true,
            slots: HashMap::from([(slot_name.clone().into(), content)]),
        };
        // Per-slot merge key. The auto-injected subscribe form (when present)
        // emits into footer-end via `__native_moss_subscribe_form_default` —
        // a different slot, different key, no conflict.
        let key = format!("__native_moss_footer_{}", slot_name);
        slots.merge(&result, 6, &key);
    }

    slots
}

/// Minimal article info needed for native feature slot generation.
/// This is a subset of the full article map data.
pub struct ArticleInfo {
    pub url_path: String,
    pub title: String,
    pub uid: String,
    pub review_of: Option<String>,
    pub rating: Option<u8>,
    pub cover: Option<String>,
    pub comments: Option<bool>,
    pub lang: Option<String>,
    /// Syndicated publication URLs from frontmatter `syndicated:` field.
    /// Used to render "Reply on X ↗" link-outs for non-Artalk source comments.
    pub syndicated: Vec<String>,
}

#[cfg(test)]
#[path = "features_tests.rs"]
mod tests;
