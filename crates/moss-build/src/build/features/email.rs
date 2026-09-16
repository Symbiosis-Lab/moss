//! Email newsletter feature: renders subscribe form in footer.
//!
//! Currently supports Buttondown only. The subscribe form posts directly
//! to Buttondown's embed API — no client-side JS needed.

use super::{html_escape, inactive_label, CHECK_SVG};
use crate::build::assets::paths::PathResolver;
use crate::i18n::Language;

// Whether a top-level folder is a language is decided by the SINGLE
// site-wide judge `moss_core::home::lang_tree_prefix` (backed by
// `KNOWN_LANG_SUFFIXES`), the same function the build's `has_language_trees`
// uses. There is no separate email allowlist — the email picker and the build
// recognize exactly the same languages. To add a language code, extend
// `KNOWN_LANG_SUFFIXES` in `crates/moss-core/src/home.rs`.

/// Derive the v1 scope (top-level folder) for a given page URL path.
///
/// "Scope" in v1 is the first path segment of the page's URL, but ONLY when
/// it appears in `supported_scopes` (the canonical list for THIS site).
/// This prevents `/posts/foo` or `/about/contact.html` on any site from
/// being mistagged with "posts" or "about" as a scope.
///
/// `supported_scopes` is the canonical list — populated for v1 by
/// intersecting "top-level folders containing index.html" with the ISO-639
/// allowlist (see `generate_native_slots` in features.rs). For single-lang
/// sites, supported_scopes is empty and this function always returns "".
///
/// Examples:
/// - url_path="en/posts/foo", supported_scopes=["en", "zh"] → "en"
/// - url_path="posts/foo", supported_scopes=[] → ""
/// - url_path="about/index.html", supported_scopes=[] → "" (about is NOT a scope)
/// - url_path="posts/foo", supported_scopes=["en"] → "" (no match)
/// - url_path="/", supported_scopes=["en"] → "" (root: no scope)
pub fn detect_page_scope(url_path: &str, supported_scopes: &[String]) -> String {
    if supported_scopes.is_empty() {
        return String::new();
    }
    let trimmed = url_path.trim_start_matches('/');
    let first_segment = trimmed.split('/').next().unwrap_or("");
    if supported_scopes.iter().any(|s| s == first_segment) {
        first_segment.to_string()
    } else {
        String::new()
    }
}

/// The folder-name half of the language judge: top-level folders whose NAME is a
/// recognized language code (`moss_core::home::lang_tree_prefix`, the SAME
/// function the build's `has_language_trees` uses). The folder existing — with
/// any content under it — is the signal; no `<folder>/index.html` homepage is
/// required. Returns the sorted, de-duplicated RAW folder names (what
/// `detect_page_scope` tags pages with). See [`derive_language_sections`] for
/// the full judge (this plus the translation-key signal).
pub fn derive_supported_scopes<'a, I>(url_paths: I) -> Vec<String>
where
    I: IntoIterator<Item = &'a str>,
{
    use std::collections::BTreeSet;
    let mut acc: BTreeSet<String> = BTreeSet::new();
    for url_path in url_paths {
        let trimmed = url_path.trim_start_matches('/');
        if let Some(prefix) = moss_core::home::lang_tree_prefix(trimmed) {
            acc.insert(prefix.to_string());
        }
    }
    acc.into_iter().collect()
}

/// One targetable email audience: a wire `scope` (top-level folder name, or `""`
/// for the root/default) and the language code `lang` used to render its label
/// in the UI (`langName()`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct EmailAudience {
    pub scope: String,
    pub lang: String,
}

/// THE single judge of a site's named language sections — shared by the build
/// (per-page scope tagging + native subscribe-form slots) and the email picker,
/// so the published form and the modal can never disagree.
///
/// A top-level folder is a language section when EITHER signal holds:
/// 1. **Folder name** — its name is a recognized language code
///    ([`derive_supported_scopes`] / `lang_tree_prefix`), so the folder itself
///    declares the language (`en/`, `zh-hant/`). Label = that code.
/// 2. **Translation key** — its home file shares the ROOT home's `translationKey`
///    (the author explicitly declared it a translation of the site home), even
///    when the folder name is NOT a recognized code (`english/`). Label = the
///    home's resolved language. This makes an explicit author declaration
///    sufficient on its own, not dependent on the hardcoded code list. The
///    build promotes such a home (`compute_home_overrides`) to
///    `<folder>/index.html`, which is what we match on here.
///
/// `scope` is always the raw top-level folder name (what `detect_page_scope`
/// tags pages with, so the form and the picker share one wire value). Sections
/// are sorted + de-duplicated; the folder-name language wins over the
/// translation-key language when both apply.
pub fn derive_language_sections(
    pages: &[crate::build::types::ParsedDocument],
) -> Vec<EmailAudience> {
    use std::collections::BTreeMap;
    // scope (raw folder) -> lang code.
    let mut sections: BTreeMap<String, String> = BTreeMap::new();

    // 1. Folder-name list. The folder code is its own label.
    for scope in derive_supported_scopes(pages.iter().map(|p| p.url_path.as_str())) {
        let lang = scope.to_ascii_lowercase();
        sections.entry(scope).or_insert(lang);
    }

    // 2. Translation key: a top-level folder whose home shares the root home's
    //    translationKey. The root home is the page emitted at `index.html`.
    let root_key = pages
        .iter()
        .find(|p| p.url_path == "index.html")
        .and_then(|p| p.translation_key.as_deref());
    if let Some(key) = root_key {
        for page in pages {
            if page.translation_key.as_deref() != Some(key) {
                continue;
            }
            let trimmed = page.url_path.trim_start_matches('/');
            // The translated home is emitted at `<folder>/index.html`.
            if let Some((folder, "index.html")) = trimmed.split_once('/') {
                if !folder.is_empty() {
                    sections
                        .entry(folder.to_string())
                        .or_insert_with(|| page.lang.code().to_string());
                }
            }
        }
    }

    sections
        .into_iter()
        .map(|(scope, lang)| EmailAudience { scope, lang })
        .collect()
}

/// The site's audience list: the root (wire scope `""`) first, then one entry
/// per named language section from [`derive_language_sections`].
///
/// `root_lang` is the language the BUILD resolved. A reader deriving this
/// WITHOUT a build passes `None`, which leaves the root unlabeled rather than
/// guessing a language nobody measured — the one deliberate difference between
/// the writer and the fallback reader, and the reason these were previously two
/// copies of this function on opposite sides of the crate boundary.
pub fn site_audience_list(
    root_lang: Option<&str>,
    pages: &[crate::build::types::ParsedDocument],
) -> Vec<EmailAudience> {
    let mut out = vec![EmailAudience {
        scope: String::new(),
        lang: root_lang.unwrap_or_default().to_string(),
    }];
    out.extend(derive_language_sections(pages));
    out
}

/// The audience list the last build persisted at
/// `.moss/build/site-languages.json`, or `None` when no build has written
/// one (or it is unreadable). The root entry's `lang` is the language the
/// build resolved for the site; readers that must answer without a build
/// decide their own fallback.
pub fn read_site_languages(project_root: &std::path::Path) -> Option<Vec<EmailAudience>> {
    let raw = std::fs::read_to_string(project_root.join(".moss/build/site-languages.json")).ok()?;
    serde_json::from_str::<Vec<EmailAudience>>(&raw).ok().filter(|list| !list.is_empty())
}

/// Render the Buttondown subscribe form HTML for the footer.
///
/// No production caller in Phase 1 — kept for Phase 2 (3rd-party providers
/// render path), see
/// docs/archive/2026-06-10-email-footer-mode-independent-design.md.
///
/// When `active` is false, adds `moss-service-inactive` class and an
/// `aria-label` naming the inactive state. Not `title=`: the inactive
/// element is `pointer-events: none` in preview and `display: none` on
/// the published site, so a native tooltip could never show — the state
/// only needs to reach the accessibility tree.
pub fn render_subscribe_form(username: &str, lang: Language, active: bool) -> String {
    let class = if active {
        "moss-subscribe-form".to_string()
    } else {
        "moss-subscribe-form moss-service-inactive".to_string()
    };
    let title_attr = if active {
        String::new()
    } else {
        format!(" aria-label=\"{}\"", inactive_label(lang))
    };
    format!(
        "<form action=\"https://buttondown.com/api/emails/embed-subscribe/{}\" method=\"post\" class=\"{}\" data-position=\"footer\"{}>\
            <input type=\"email\" name=\"email\" class=\"moss-input\" placeholder=\"{}\" required />\
            <input type=\"hidden\" value=\"1\" name=\"embed\" />\
            <button type=\"submit\" class=\"moss-btn\">{}</button>\
        </form>",
        html_escape(username),
        class,
        title_attr,
        email_placeholder(lang),
        subscribe_label(lang),
    )
}

/// Get the placeholder text for the email input (Buttondown form).
fn email_placeholder(lang: Language) -> &'static str {
    match lang {
        Language::En => "email",
        Language::ZhHans => "邮箱",
        Language::ZhHant => "電子郵件",
    }
}

/// Get the submit button text (Buttondown form).
fn subscribe_label(lang: Language) -> &'static str {
    match lang {
        Language::En => "Subscribe",
        Language::ZhHans => "订阅",
        Language::ZhHant => "訂閱",
    }
}

/// Fetch the Buttondown newsletter username from the API.
///
/// No production caller in Phase 1 — the build path no longer fetches
/// (docs/archive/2026-06-10-email-footer-mode-independent-design.md). Kept,
/// with its bounded-timeout test, for Phase 2's Settings-time wiring
/// acquisition, which persists the username into `[services.email]`.
pub fn fetch_buttondown_username(api_key: &str) -> Option<String> {
    let base = std::env::var("MOSS_BUTTONDOWN_API_BASE")
        .unwrap_or_else(|_| "https://api.buttondown.com".to_string());
    let url = format!("{}/v1/newsletters", base);
    // Bound TCP connect time so an unreachable host can't tie up the build.
    // See #576. Both the connect timeout and overall timeout apply.
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(std::time::Duration::from_secs(3))
        .timeout(std::time::Duration::from_secs(10))
        .build();
    let resp = agent
        .get(&url)
        .set("Authorization", &format!("Token {}", api_key))
        .call()
        .ok()?;
    let json: serde_json::Value = resp.into_json().ok()?;
    // Try results array first (paginated response), then root object
    json.get("results")
        .and_then(|r| r.as_array())
        .and_then(|a| a.first())
        .or(Some(&json))
        .and_then(|obj| obj.get("username"))
        .and_then(|u| u.as_str())
        .map(|s| s.to_string())
}

/// Copy set for the moss-hosted subscribe form.
///
/// Must stay in sync with `COPY` in `frontend/site/subscribe/i18n.ts`. The
/// set is small enough (5 strings × 3 locales) that duplication beats
/// plumbing the strings as data- attributes through the DOM.
struct MossSubscribeCopy {
    placeholder: &'static str,
    label: &'static str,
    check_email: &'static str,
    already_subscribed: &'static str,
    error: &'static str,
}

fn moss_subscribe_copy(lang: Language) -> MossSubscribeCopy {
    match lang {
        Language::En => MossSubscribeCopy {
            placeholder: "your@email.com",
            label: "Subscribe",
            check_email: "Check your email to confirm.",
            already_subscribed: "You're already subscribed.",
            error: "Something went wrong. Try again?",
        },
        Language::ZhHans => MossSubscribeCopy {
            placeholder: "邮箱",
            label: "订阅",
            check_email: "请在邮箱中确认订阅",
            already_subscribed: "你已订阅。",
            error: "出错了，请重试。",
        },
        Language::ZhHant => MossSubscribeCopy {
            placeholder: "電子郵件",
            label: "訂閱",
            check_email: "請在電子郵件中確認訂閱",
            already_subscribed: "你已訂閱。",
            error: "出錯了，請重試。",
        },
    }
}

/// Render the input/button/status spans that go inside the subscribe form.
///
/// Shared by both the footer renderer (`render_moss_subscribe_form`) and the
/// inline shortcode renderer (`render_inline_subscribe_form`). Returns the
/// inner HTML only — the caller wraps it in a `<form>` element.
///
/// `placeholder_override` and `button_override`, when `Some` and non-empty,
/// replace the language-default copy. Empty strings fall back to the default.
fn subscribe_form_inner(
    lang: Language,
    placeholder_override: Option<&str>,
    button_override: Option<&str>,
) -> String {
    let c = moss_subscribe_copy(lang);
    let label = match button_override {
        Some(s) if !s.is_empty() => html_escape(s),
        _ => c.label.to_string(),
    };
    let placeholder = match placeholder_override {
        Some(s) if !s.is_empty() => html_escape(s),
        _ => c.placeholder.to_string(),
    };
    let envelope_svg = r##"<svg class="moss-subscribe-status__icon" viewBox="0 0 24 24" aria-hidden="true"><path d="M4 7l8 6 8-6M4 7v10a1 1 0 0 0 1 1h14a1 1 0 0 0 1-1V7M4 7l1-1h14l1 1" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/></svg>"##;
    format!(
        r#"<input type="email" name="email" class="moss-input" placeholder="{placeholder}" required>
  <span class="moss-btn-slot">
    <button type="submit" class="moss-btn">
      <span class="moss-btn__label">{label}</span>
      <span class="moss-btn__spinner" aria-hidden="true"></span>
      <span class="moss-btn__check" aria-hidden="true">{check_svg}</span>
    </button>
  </span>
  <div class="moss-subscribe-status" aria-live="polite">
    {envelope_svg}
    <span data-copy="check-email" hidden>{check_email}</span>
    <span data-copy="already-subscribed" hidden>{already}</span>
    <span data-copy="error" hidden>{error}</span>
  </div>"#,
        placeholder = placeholder,
        label = label,
        check_email = c.check_email,
        already = c.already_subscribed,
        error = c.error,
        envelope_svg = envelope_svg,
        check_svg = CHECK_SVG,
    )
}

/// The single moss-hosted subscribe form (the `:::subscribe` shortcode shape).
///
/// This is the ONE renderer for every moss-hosted subscribe form: the
/// auto-injected "default email footer" is just this shortcode injected into
/// the `footer-end` slot, and `:::subscribe` in body/footer.md renders the
/// identical HTML. (Supersedes the split `render_moss_subscribe_form` /
/// `render_inline_subscribe_form` pair.)
///
/// - `site_id`: `Some(non-empty)` → wired (`action=/sites/{id}/subscribe`);
///   `None`/empty → pending (`action="#"` + `data-moss-pending-site`), shown
///   before first publish (subscribe.ts runs the local success animation).
/// - `button_override`: `Some(non-empty)` emits `data-button-override="true"`
///   so subscribe.ts leaves the author's button label (and placeholder) alone.
///
/// Public contract (see `docs/reference/html-css-contract.md`):
/// - classes `moss-subscribe`, `moss-subscribe-form`
/// - data-position `"inline"`; data-moss-hosted `"true"`
/// - data-state value space `"idle" | "loading" | "success" | "error"`
pub fn render_hosted_subscribe_form(
    site_id: Option<&str>,
    lang: Language,
    scope: &str,
    placeholder_override: Option<&str>,
    button_override: Option<&str>,
    seta_base: &str,
) -> String {
    let inner = subscribe_form_inner(lang, placeholder_override, button_override);
    let site = site_id.map(str::trim).filter(|s| !s.is_empty());
    let (pending_attr, action) = match site {
        Some(id) => (
            String::new(),
            format!("{}/sites/{}/subscribe", seta_base, html_escape(id)),
        ),
        None => (
            r#" data-moss-pending-site="true""#.to_string(),
            "#".to_string(),
        ),
    };
    let override_attr = match button_override.map(str::trim).filter(|s| !s.is_empty()) {
        Some(_) => r#" data-button-override="true""#,
        None => "",
    };
    format!(
        r#"<div class="moss-subscribe" data-state="idle">
  <form class="moss-subscribe-form" data-position="inline" data-moss-hosted="true"{pending_attr}{override_attr} method="post" action="{action}">
    <input type="hidden" name="scope" value="{scope}">
  {inner}
</form>
</div>"#,
        pending_attr = pending_attr,
        override_attr = override_attr,
        action = action,
        scope = html_escape(scope),
        inner = inner,
    )
}

/// Copy set for the moss-hosted apply form.
///
/// Must stay in sync with `ApplyCopy` in `frontend/site/subscribe/i18n.ts`. The
/// set is small (11 strings × 3 locales) — duplication beats DOM plumbing.
struct MossApplyCopy {
    /// Accessible name for the email input (rendered as `aria-label`; the field
    /// is placeholder-only, no visible `<label>`).
    email_label: &'static str,
    email_ph: &'static str,
    email_helper: &'static str,
    /// Accessible name for the second field (rendered as `aria-label`).
    matters_label: &'static str,
    /// Placeholder for the second field — a Matters username OR a one-line pitch.
    /// Must stay in sync with `matters_ph` in `frontend/site/subscribe/i18n.ts`.
    matters_ph: &'static str,
    /// Helper line beneath the second field: the "or tell us what you write" hint.
    matters_helper: &'static str,
    label: &'static str,
    label_success: &'static str,
    received: &'static str,
    error: &'static str,
}

fn moss_apply_copy(lang: Language) -> MossApplyCopy {
    match lang {
        Language::En => MossApplyCopy {
            email_label: "Email",
            email_ph: "your@email.com",
            email_helper: "For your invitation and free hosting",
            matters_label: "Matters username",
            matters_ph: "Matters username",
            matters_helper: "Or tell us what you plan to write — a line is plenty.",
            label: "Apply",
            label_success: "Application sent",
            received: "Thanks for applying. Please confirm the email we just sent; we'll email you when your turn comes.",
            error: "Something went wrong. Try again?",
        },
        Language::ZhHans => MossApplyCopy {
            email_label: "邮箱",
            email_ph: "邮箱",
            email_helper: "用于获取邀请及免费托管服务",
            matters_label: "Matters 用户名",
            matters_ph: "Matters 用户名",
            matters_helper: "或者告诉我们你打算写什么，一句话就好",
            label: "申请",
            label_success: "申请已提交",
            received: "谢谢你的申请，请查收确认邮件。我们会在轮到你时通过邮件邀请。",
            error: "出错了，请重试。",
        },
        Language::ZhHant => MossApplyCopy {
            email_label: "電子郵件",
            email_ph: "電子郵件",
            email_helper: "用於獲取邀請及免費託管服務",
            matters_label: "Matters 用戶名",
            matters_ph: "Matters 用戶名",
            matters_helper: "或者告訴我們你打算寫什麼，一句話就好",
            label: "申請",
            label_success: "申請已提交",
            received: "謝謝你的申請，請查收確認郵件。我們會在輪到你時透過郵件邀請。",
            error: "出錯了，請重試。",
        },
    }
}

/// Render the inner fields+button for the apply form.
///
/// Shared by `render_inline_apply_form`. Returns the inner HTML only —
/// the caller wraps it in the outer `<div class="moss-apply">` and `<form>`.
fn apply_form_inner(
    lang: Language,
    placeholder_override: Option<&str>,
    button_override: Option<&str>,
) -> String {
    let c = moss_apply_copy(lang);
    let label = match button_override {
        Some(s) if !s.is_empty() => html_escape(s),
        _ => c.label.to_string(),
    };
    let email_ph = match placeholder_override {
        Some(s) if !s.is_empty() => html_escape(s),
        _ => c.email_ph.to_string(),
    };
    let check_svg = r##"<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M5 12.5l4.5 4.5L19 7" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"/></svg>"##;
    // NOTE: Static ids (moss-apply-email, moss-apply-matters) assume one :::apply per page.
    // If multiple forms appear on the same page, for= / id= pairing still works but ids
    // are not unique — acceptable for this single-form use case.
    // The matters field carries either a handle or a one-line pitch. seta caps it
    // by FAIR weighted length (CJK/wide = 2, Latin = 1), max 300 weighted units —
    // which no HTML maxlength can express. maxlength=300 is the tightest ceiling
    // that still can't truncate a valid entry: the server keeps at most 300 UTF-16
    // code units (the all-Latin worst case of the weighted-300 cap), so a full
    // Latin pitch fits exactly and shorter CJK input stays well within budget.
    format!(
        r#"<input type="email" id="moss-apply-email" name="email" class="moss-input" placeholder="{email_ph}" inputmode="email" autocomplete="email" required aria-label="{email_label}" aria-describedby="moss-apply-email-help">
    <p class="moss-apply-helper" id="moss-apply-email-help">{email_helper}</p>
    <input type="text" id="moss-apply-matters" name="matters" class="moss-input moss-apply-matters" placeholder="{matters_ph}" aria-label="{matters_label}" aria-describedby="moss-apply-matters-help" maxlength="300">
    <p class="moss-apply-helper" id="moss-apply-matters-help">{matters_helper}</p>
    <input type="text" name="website" class="moss-apply-hp" tabindex="-1" autocomplete="off" aria-hidden="true">
    <span class="moss-btn-slot">
      <button type="submit" class="moss-btn">
        <span class="moss-btn__label" data-label-success="{label_success}">{label}</span>
        <span class="moss-btn__spinner" aria-hidden="true"></span>
        <span class="moss-btn__check" aria-hidden="true">{check_svg}</span>
      </button>
    </span>
    <div class="moss-subscribe-status moss-apply-status" aria-live="polite" role="status" tabindex="-1">
      <span data-copy="received" hidden>{received}</span>
      <span data-copy="error" hidden>{error}</span>
    </div>"#,
        email_ph = email_ph,
        email_label = html_escape(c.email_label),
        email_helper = html_escape(c.email_helper),
        matters_ph = html_escape(c.matters_ph),
        matters_label = html_escape(c.matters_label),
        matters_helper = html_escape(c.matters_helper),
        label = label,
        label_success = html_escape(c.label_success),
        received = c.received,
        error = c.error,
        check_svg = check_svg,
    )
}

/// Render the inline `:::apply` shortcode form.
///
/// Wraps the shared `apply_form_inner` HTML in a `<div class="moss-apply">` plus
/// inner `<form>`. The form POSTs `application/x-www-form-urlencoded` to
/// `{seta_base}/apply?lang={lang}` with fields `email`, `matters`, `scope`,
/// and `website` (honeypot). No `ts`/`token` — the no-JS native form
/// can't mint them (see Task 0 in the plan).
///
/// `data-revert="false"` tells subscribe.ts to use the terminal (non-reverting)
/// success flow — label stays as `申请已提交` ink pill, no auto-reset.
///
/// Public contract surface:
/// - classes: `moss-apply`, `moss-subscribe-form moss-apply-form`
/// - data-position: `"apply"`
/// - data-revert: `"false"`
/// - data-moss-hosted: `"true"`
pub fn render_inline_apply_form(
    _site_id: &str,
    lang: Language,
    scope: &str,
    placeholder_override: Option<&str>,
    button_override: Option<&str>,
    seta_base: &str,
) -> String {
    let lang_str = match lang {
        Language::En => "en",
        Language::ZhHans => "zh-hans",
        Language::ZhHant => "zh-hant",
    };
    let inner = apply_form_inner(lang, placeholder_override, button_override);
    format!(
        r#"<div class="moss-apply" data-state="idle">
  <form class="moss-subscribe-form moss-apply-form" data-position="apply" data-moss-hosted="true" data-revert="false" data-state="idle" method="post" action="{seta_base}/apply?lang={lang_str}">
    <input type="hidden" name="scope" value="{scope}">
    {inner}
  </form>
</div>"#,
        seta_base = seta_base,
        lang_str = lang_str,
        scope = html_escape(scope),
        inner = inner,
    )
}

/// The bundled subscribe.js IIFE — produced by vite.config.js from
/// `frontend/site/subscribe/subscribe.ts`. Embed once per page that
/// contains a moss-hosted subscribe form.
pub const SUBSCRIBE_JS: &str = include_str!("../../assets/js/subscribe.js");

/// The site-local URL path seta redirects readers to after a successful
/// double-opt-in confirmation. Wire contract — must stay in sync with
/// `moss-seta/src/routes/subscribers.ts::canonicalSiteUrl` (the
/// redirect target is built as `{siteUrl}{CONFIRMED_REDIRECT_PATH}`).
///
/// Grepping for this constant turns up both ends of the contract:
/// the moss-side emitter and the seta-side redirect caller.
pub const CONFIRMED_REDIRECT_PATH: &str = "/subscribe/confirmed";

/// Sibling path for the expired/invalid-token branch. Seta currently
/// renders its own inline expired page instead of redirecting, but
/// we emit a landing page here so the path is ready if seta ever
/// starts redirecting that branch too.
pub const EXPIRED_REDIRECT_PATH: &str = "/subscribe/expired";

/// Which landing page to render — returned to the reader after they click
/// the double-opt-in link in their email.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubscribeLanding {
    /// Shown after seta confirms the token — reader is now fully subscribed.
    Confirmed,
    /// Shown when seta's token check fails (expired / already used / bogus).
    /// Seta currently renders its own inline expired page when it can't
    /// redirect safely; having this file on the site side avoids a 404 if
    /// seta ever starts redirecting the expired branch too.
    Expired,
}

/// Render a self-contained landing page (confirmed / expired) for the
/// site's `/subscribe/<kind>/index.html` path.
///
/// The page loads the site's own stylesheet at `css_href` so it inherits
/// typography and color tokens, with a minimal inline style block as layout
/// fallback. `css_href` must be the build's resolved path — a published build
/// emits `/_moss/style.<hash>.css` and has no unhashed `/_moss/style.css`, so
/// hardcoding the unhashed name leaves these two pages unstyled.
/// Copy is localized via `Language::from_bcp47_lenient`.
pub fn render_subscribe_landing_html(
    lang: Language,
    kind: SubscribeLanding,
    css_href: &str,
) -> String {
    let (title, heading, body) = landing_copy(lang, kind);
    let back_label = landing_back_label(lang);
    let html_lang = match lang {
        Language::ZhHans => "zh-Hans",
        Language::ZhHant => "zh-Hant",
        Language::En => "en",
    };
    format!(
        r#"<!DOCTYPE html>
<html lang="{html_lang}">
<head>
  <meta charset="UTF-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  <meta name="robots" content="noindex, nofollow" />
  <title>{title}</title>
  <link rel="stylesheet" href="{css_href}" />
  <style>
    .moss-subscribe-landing {{
      max-width: 36rem;
      margin: 12vh auto;
      padding: 2.5rem 1.5rem;
      text-align: center;
    }}
    .moss-subscribe-landing h1 {{
      margin: 0 0 0.5em;
      font-size: 1.4rem;
      font-weight: 500;
    }}
    .moss-subscribe-landing p {{
      color: var(--moss-color-muted, #716d69);
      margin: 0 0 1.5em;
      font-size: 1rem;
    }}
    .moss-subscribe-landing a {{
      color: var(--moss-color-accent, #2d5a2d);
      text-decoration: none;
      border-bottom: 1px solid currentColor;
    }}
  </style>
</head>
<body>
  <main class="moss-subscribe-landing">
    <h1>{heading}</h1>
    <p>{body}</p>
    <p><a href="/">{back_label}</a></p>
  </main>
</body>
</html>
"#
    )
}

fn landing_copy(
    lang: Language,
    kind: SubscribeLanding,
) -> (&'static str, &'static str, &'static str) {
    match (lang, kind) {
        (Language::En, SubscribeLanding::Confirmed) => (
            "Subscribed",
            "You're subscribed",
            "You'll get an email when new writing is posted. Thanks for reading.",
        ),
        (Language::En, SubscribeLanding::Expired) => (
            "Link expired",
            "This link has expired",
            "Subscription links are valid for 48 hours. Please subscribe again from the site.",
        ),
        (Language::ZhHans, SubscribeLanding::Confirmed) => (
            "已订阅",
            "订阅成功",
            "新内容发布时会发邮件通知你。感谢阅读。",
        ),
        (Language::ZhHans, SubscribeLanding::Expired) => (
            "链接已过期",
            "链接已过期",
            "订阅链接有效期为四十八小时，请重新在网站上订阅。",
        ),
        (Language::ZhHant, SubscribeLanding::Confirmed) => (
            "已訂閱",
            "訂閱成功",
            "有新內容發佈時會寄信通知你。感謝閱讀。",
        ),
        (Language::ZhHant, SubscribeLanding::Expired) => (
            "連結已失效",
            "連結已失效",
            "訂閱連結有效期為四十八小時，請重新在網站上訂閱。",
        ),
    }
}

fn landing_back_label(lang: Language) -> &'static str {
    match lang {
        Language::En => "← Back to the site",
        Language::ZhHans => "← 返回网站",
        Language::ZhHant => "← 返回網站",
    }
}

/// All landing pages to emit for a moss-hosted site's output directory.
/// Each tuple is `(relative_path, html)` — paths are forward-slash so
/// the caller can `output_dir.join()` them cross-platform.
///
/// Paths are derived from `CONFIRMED_REDIRECT_PATH` / `EXPIRED_REDIRECT_PATH`
/// so the wire contract with seta has a single source of truth.
/// `css_version` is the build's stylesheet content hash (empty in
/// preview/watch mode), resolved here the same way the rendered pages
/// resolve theirs.
pub fn render_subscribe_landing_pages(lang: Language, css_version: &str) -> Vec<(String, String)> {
    let css_href = match css_version {
        "" => PathResolver::new(),
        version => PathResolver::new().with_css_version(version),
    }
    .css_path();
    let css_href = css_href.as_str();
    vec![
        (
            format!("{}/index.html", CONFIRMED_REDIRECT_PATH.trim_start_matches('/')),
            render_subscribe_landing_html(lang, SubscribeLanding::Confirmed, css_href),
        ),
        (
            format!("{}/index.html", EXPIRED_REDIRECT_PATH.trim_start_matches('/')),
            render_subscribe_landing_html(lang, SubscribeLanding::Expired, css_href),
        ),
    ]
}


#[cfg(test)]
mod tests {
    use super::*;

    /// A page at `<scope>/...` with the given translation key (helper for the
    /// `derive_language_sections` tests).
    fn page(url_path: &str, translation_key: Option<&str>, lang: Language) -> crate::build::types::ParsedDocument {
        crate::build::types::ParsedDocument {
            url_path: url_path.to_string(),
            translation_key: translation_key.map(|s| s.to_string()),
            lang,
            ..Default::default()
        }
    }

    #[test]
    fn derive_supported_scopes_mints_any_language_folder_with_content() {
        // A top-level folder named a language code mints its scope from the mere
        // presence of content — NO <folder>/index.html homepage required (this is
        // the 刘果 case: `de/posts/c.html` and `en/posts/a.html` both count).
        // /about/ and /posts/ are not language codes → not scopes. Output is the
        // sorted, de-duplicated RAW folder names.
        let urls = [
            "index.html",
            "en/index.html",
            "en/posts/a.html", // dedup with the above
            "zh-hans/index.html",
            "about/index.html",
            "posts/b.html",
            "de/posts/c.html", // de/ has content but no de/index.html homepage
        ];
        let scopes = derive_supported_scopes(urls.iter().copied());
        assert_eq!(
            scopes,
            vec!["de".to_string(), "en".to_string(), "zh-hans".to_string()]
        );
    }

    #[test]
    fn derive_supported_scopes_empty_for_single_language_site() {
        let urls = ["index.html", "posts/a.html", "about/index.html"];
        assert!(derive_supported_scopes(urls.iter().copied()).is_empty());
    }

    #[test]
    fn language_sections_from_folder_name_only() {
        // No translation keys: sections come purely from lang-code folder names.
        let pages = [
            page("index.html", None, Language::ZhHans),
            page("en/posts/a.html", None, Language::En),
        ];
        assert_eq!(
            derive_language_sections(&pages),
            vec![EmailAudience { scope: "en".into(), lang: "en".into() }]
        );
    }

    #[test]
    fn language_sections_from_translation_key_for_non_code_folder() {
        // The English home lives in `english/` — NOT a recognized code — but its
        // home shares the root home's translationKey, so it is still a section.
        // Label comes from the home's resolved language, not the folder name.
        let pages = [
            page("index.html", Some("home"), Language::ZhHans),
            page("english/index.html", Some("home"), Language::En),
            page("english/posts/a.html", None, Language::En),
        ];
        assert_eq!(
            derive_language_sections(&pages),
            vec![EmailAudience { scope: "english".into(), lang: "en".into() }]
        );
    }

    #[test]
    fn language_sections_folder_name_lang_wins_over_translation_key() {
        // A code-named folder that is ALSO translation-keyed keeps its folder
        // code as the label (folder-name pass wins); no duplicate section.
        let pages = [
            page("index.html", Some("home"), Language::ZhHans),
            page("en/index.html", Some("home"), Language::En),
        ];
        assert_eq!(
            derive_language_sections(&pages),
            vec![EmailAudience { scope: "en".into(), lang: "en".into() }]
        );
    }

    // --- Scope input (Task 1.6) ---
    //
    // Both subscribe-form renderers must emit a hidden <input name="scope">
    // so the seta /subscribe endpoint can bucket subscribers by their page's
    // top-level folder ("en", "zh"). The lang parameter remains for UI-copy
    // bucketing — orthogonal to audience scope.

    #[test]
    fn moss_form_emits_hidden_scope_input() {
        let html = render_hosted_subscribe_form(Some("site-abc"), Language::En, "en", None, None, "https://api.mosspub.com");
        assert!(
            html.contains(r#"<input type="hidden" name="scope" value="en""#),
            "expected hidden scope input, got: {html}",
        );
    }

    #[test]
    fn moss_form_emits_empty_hidden_scope_for_single_lang_site() {
        let html = render_hosted_subscribe_form(Some("site-abc"), Language::En, "", None, None, "https://api.mosspub.com");
        assert!(
            html.contains(r#"<input type="hidden" name="scope" value=""#),
            "expected hidden scope input even when empty: {html}",
        );
    }

    #[test]
    fn inline_form_emits_hidden_scope_input() {
        let html = render_hosted_subscribe_form(Some("site-abc"), Language::ZhHans, "zh", None, None, "https://api.mosspub.com");
        assert!(
            html.contains(r#"<input type="hidden" name="scope" value="zh""#),
            "expected hidden scope input on inline form, got: {html}",
        );
    }

    #[test]
    fn scope_input_html_escapes_value() {
        // Scope normally is a filesystem-name; defense in depth.
        let html = render_hosted_subscribe_form(Some("site-abc"), Language::En, r#""><script>"#, None, None, "https://api.mosspub.com");
        assert!(!html.contains(r#""><script>"#), "must escape: {html}");
        assert!(html.contains("&quot;&gt;&lt;script&gt;"));
    }

    #[test]
    fn test_render_form_contains_action_url() {
        let html = render_subscribe_form("liuguo", Language::ZhHans, true);
        assert!(html.contains("https://buttondown.com/api/emails/embed-subscribe/liuguo"));
    }

    #[test]
    fn test_render_form_contains_email_input() {
        let html = render_subscribe_form("liuguo", Language::ZhHans, true);
        assert!(html.contains("type=\"email\""));
        assert!(html.contains("name=\"email\""));
        assert!(html.contains("class=\"moss-input\""));
        assert!(html.contains("required"));
    }

    #[test]
    fn test_render_form_contains_hidden_embed() {
        let html = render_subscribe_form("liuguo", Language::ZhHans, true);
        assert!(html.contains("type=\"hidden\""));
        assert!(html.contains("name=\"embed\""));
        assert!(html.contains("value=\"1\""));
    }

    #[test]
    fn test_render_form_contains_submit_button() {
        let html = render_subscribe_form("liuguo", Language::ZhHans, true);
        assert!(html.contains("type=\"submit\""));
        assert!(html.contains("class=\"moss-btn\""));
    }

    #[test]
    fn test_render_form_zh_labels() {
        let html = render_subscribe_form("liuguo", Language::ZhHans, true);
        assert!(html.contains("placeholder=\"邮箱\""));
        assert!(html.contains("订阅"));
    }

    #[test]
    fn test_render_form_en_labels() {
        let html = render_subscribe_form("testuser", Language::En, true);
        assert!(html.contains("placeholder=\"email\""));
        assert!(html.contains("Subscribe"));
    }

    #[test]
    fn test_render_form_structure() {
        let html = render_subscribe_form("liuguo", Language::ZhHans, true);
        assert!(html.contains("class=\"moss-subscribe-form\""));
        assert!(html.contains("data-position=\"footer\""));
        assert!(html.starts_with("<form"));
        assert!(html.ends_with("</form>"));
    }

    #[test]
    fn test_render_form_inactive_has_class_and_label() {
        let html = render_subscribe_form("liuguo", Language::En, false);
        assert!(html.contains("moss-service-inactive"), "Inactive form should have moss-service-inactive class");
        assert!(html.contains("aria-label=\"Available after publishing\""), "Inactive form should name its state");
    }

    #[test]
    fn test_render_form_inactive_zh_label() {
        let html = render_subscribe_form("liuguo", Language::ZhHans, false);
        assert!(html.contains("moss-service-inactive"));
        assert!(html.contains("aria-label=\"发布后启用\""));
    }

    #[test]
    fn test_render_form_active_no_inactive_class() {
        let html = render_subscribe_form("liuguo", Language::En, true);
        assert!(!html.contains("moss-service-inactive"), "Active form should NOT have inactive class");
    }

    #[test]
    fn test_email_placeholder_zh() {
        assert_eq!(email_placeholder(Language::ZhHans), "邮箱");
        assert_eq!(email_placeholder(Language::ZhHant), "電子郵件");
    }

    #[test]
    fn test_email_placeholder_en() {
        assert_eq!(email_placeholder(Language::En), "email");
    }

    #[test]
    fn test_subscribe_label_zh() {
        assert_eq!(subscribe_label(Language::ZhHans), "订阅");
        assert_eq!(subscribe_label(Language::ZhHant), "訂閱");
    }

    #[test]
    fn test_subscribe_label_en() {
        assert_eq!(subscribe_label(Language::En), "Subscribe");
    }

    // --- Moss-hosted subscribe form ---

    #[test]
    fn test_moss_form_uses_footer_subscribe_class() {
        let html = render_hosted_subscribe_form(Some("test-site"), Language::En, "", None, None, "https://api.mosspub.com");
        assert!(html.contains("class=\"moss-subscribe-form\""),
            "Moss form should use moss-subscribe-form class for shared CSS: {html}");
        assert!(html.contains("data-position=\"inline\""),
            "Consolidated hosted form carries data-position=\"inline\": {html}");
    }

    #[test]
    fn test_moss_form_input_has_moss_input_class() {
        let html = render_hosted_subscribe_form(Some("test-site"), Language::En, "", None, None, "https://api.mosspub.com");
        assert!(html.contains("class=\"moss-input\""),
            "Moss form input should have moss-input class: {html}");
    }

    #[test]
    fn test_moss_form_button_has_moss_btn_class() {
        let html = render_hosted_subscribe_form(Some("test-site"), Language::En, "", None, None, "https://api.mosspub.com");
        assert!(html.contains("class=\"moss-btn\""),
            "Moss form button should have moss-btn class: {html}");
    }

    #[test]
    fn test_moss_form_button_wrapped_in_slot() {
        // Slot keeps the input's flex calculation stable across state changes.
        // If this wrapper disappears, the button-to-circle morph will shift the
        // input horizontally on success.
        let html = render_hosted_subscribe_form(Some("test-site"), Language::En, "", None, None, "https://api.mosspub.com");
        assert!(
            html.contains("class=\"moss-btn-slot\""),
            "Submit button must be wrapped in .moss-btn-slot: {html}"
        );
    }

    #[test]
    fn test_moss_form_en_labels() {
        let html = render_hosted_subscribe_form(Some("test-site"), Language::En, "", None, None, "https://api.mosspub.com");
        assert!(html.contains("placeholder=\"your@email.com\""));
        assert!(html.contains("Subscribe"));
    }

    #[test]
    fn test_moss_form_zh_hans_labels() {
        let html = render_hosted_subscribe_form(Some("test-site"), Language::ZhHans, "", None, None, "https://api.mosspub.com");
        assert!(html.contains("placeholder=\"邮箱\""));
        assert!(html.contains("订阅"));
        assert!(html.contains("请在邮箱中确认订阅"));
    }

    #[test]
    fn test_moss_form_zh_hant_labels() {
        let html = render_hosted_subscribe_form(Some("test-site"), Language::ZhHant, "", None, None, "https://api.mosspub.com");
        assert!(html.contains("placeholder=\"電子郵件\""), "Traditional should use 電子郵件, not 邮箱: {html}");
        assert!(html.contains("訂閱"), "Traditional label should be 訂閱: {html}");
        assert!(html.contains("請在電子郵件中確認訂閱"));
    }

    #[test]
    fn test_moss_form_data_state_idle() {
        let html = render_hosted_subscribe_form(Some("test-site"), Language::En, "", None, None, "https://api.mosspub.com");
        assert!(html.contains("data-moss-hosted=\"true\""),
            "Form must carry data-moss-hosted for subscribe.ts to hydrate: {html}");
        assert!(html.contains("data-state=\"idle\""));
    }

    #[test]
    fn test_moss_form_status_spans_present() {
        let html = render_hosted_subscribe_form(Some("test-site"), Language::En, "", None, None, "https://api.mosspub.com");
        assert!(html.contains("data-copy=\"check-email\""));
        assert!(html.contains("data-copy=\"already-subscribed\""));
        assert!(html.contains("data-copy=\"error\""));
    }

    #[test]
    fn test_moss_form_lang_bucket_via_language_enum() {
        // This is the Rust side of the sync contract with frontend/site/subscribe/i18n.ts.
        // Any change here needs a mirroring change in langBucket() over there.
        use crate::i18n::Language;
        assert_eq!(Language::from_bcp47_lenient("en"), Language::En);
        assert_eq!(Language::from_bcp47_lenient(""), Language::En);
        assert_eq!(Language::from_bcp47_lenient("fr"), Language::En);
        assert_eq!(Language::from_bcp47_lenient("zh-hans"), Language::ZhHans);
        assert_eq!(Language::from_bcp47_lenient("zh-CN"), Language::ZhHans);
        assert_eq!(Language::from_bcp47_lenient("zh"), Language::ZhHans);
        assert_eq!(Language::from_bcp47_lenient("zh-Hant"), Language::ZhHant);
        assert_eq!(Language::from_bcp47_lenient("zh-TW"), Language::ZhHant);
        assert_eq!(Language::from_bcp47_lenient("zh-HK"), Language::ZhHant);
        assert_eq!(Language::from_bcp47_lenient("zh-MO"), Language::ZhHant);
    }

    #[test]
    fn test_landing_page_emits_two_paths() {
        let pages = render_subscribe_landing_pages(Language::En, "abc123");
        let paths: Vec<&str> = pages.iter().map(|(p, _)| p.as_str()).collect();
        assert!(paths.contains(&"subscribe/confirmed/index.html"), "paths: {:?}", paths);
        assert!(paths.contains(&"subscribe/expired/index.html"), "paths: {:?}", paths);
    }

    #[test]
    fn test_landing_page_paths_derive_from_redirect_consts() {
        // Sync contract: the subscribe landing files must sit at the paths
        // seta redirects to. If either side changes, the other must too.
        let pages = render_subscribe_landing_pages(Language::En, "abc123");
        assert!(pages.iter().any(|(p, _)| p.starts_with(
            CONFIRMED_REDIRECT_PATH.trim_start_matches('/'))));
        assert!(pages.iter().any(|(p, _)| p.starts_with(
            EXPIRED_REDIRECT_PATH.trim_start_matches('/'))));
    }

    #[test]
    fn test_landing_page_loads_site_stylesheet() {
        // A published build emits only `/_moss/style.<hash>.css`, so linking
        // the unhashed name left these two pages unstyled on every deployed
        // site. Preview/watch (empty version) keeps the unhashed name.
        for (_, html) in render_subscribe_landing_pages(Language::En, "abc123") {
            assert!(
                html.contains(r#"<link rel="stylesheet" href="/_moss/style.abc123.css""#),
                "Landing page must link the build's hashed stylesheet: {html}"
            );
        }
        for (_, html) in render_subscribe_landing_pages(Language::En, "") {
            assert!(html.contains(r#"<link rel="stylesheet" href="/_moss/style.css""#));
        }
    }

    #[test]
    fn test_landing_page_en_copy() {
        let html = render_subscribe_landing_html(Language::En, SubscribeLanding::Confirmed, "/_moss/style.css");
        assert!(html.contains("You're subscribed"));
        assert!(html.contains("Back to the site"));
        let html = render_subscribe_landing_html(Language::En, SubscribeLanding::Expired, "/_moss/style.css");
        assert!(html.contains("Link expired") || html.contains("This link has expired"));
    }

    #[test]
    fn test_landing_page_zh_hans_copy() {
        let html = render_subscribe_landing_html(Language::ZhHans, SubscribeLanding::Confirmed, "/_moss/style.css");
        assert!(html.contains("订阅成功"), "Simplified zh heading: {html}");
        assert!(html.contains("lang=\"zh-Hans\""));
    }

    #[test]
    fn test_landing_page_zh_hant_copy() {
        let html = render_subscribe_landing_html(Language::ZhHant, SubscribeLanding::Confirmed, "/_moss/style.css");
        assert!(html.contains("訂閱成功"), "Traditional zh heading: {html}");
        assert!(html.contains("lang=\"zh-Hant\""));
    }

    #[test]
    fn test_landing_page_noindex() {
        // Search engines have no reason to index these pages, and the
        // confirmed page reveals an email address in its query params.
        let html = render_subscribe_landing_html(Language::En, SubscribeLanding::Confirmed, "/_moss/style.css");
        assert!(html.contains(r#"<meta name="robots" content="noindex"#));
    }

    #[test]
    fn test_subscribe_js_embed_not_empty() {
        // Sanity: the vite build must have produced the bundled subscribe.js.
        // Empty file would silently leave subscribe forms un-hydrated in prod.
        assert!(
            !SUBSCRIBE_JS.trim().is_empty(),
            "SUBSCRIBE_JS is empty — did vite run? Rebuild with `pnpm install && pnpm run build` or start `pnpm run dev`."
        );
    }

    // --- Username fetch ---

    #[test]
    fn test_subscribe_form_inner_emits_input_and_button() {
        let html = subscribe_form_inner(Language::En, None, None);
        assert!(html.contains(r#"<input type="email""#),
            "inner must contain email input: {html}");
        assert!(html.contains("<button type=\"submit\""),
            "inner must contain submit button: {html}");
        assert!(!html.contains(r#"data-state="idle""#),
            "data-state belongs on the outer form, not the inner: {html}");
    }

    #[test]
    fn test_subscribe_form_inner_with_button_override() {
        let html = subscribe_form_inner(Language::En, None, Some("Request access"));
        assert!(html.contains(">Request access<"),
            "must use override label: {html}");
    }

    #[test]
    fn test_subscribe_form_inner_with_placeholder_override() {
        let html = subscribe_form_inner(Language::En, Some("you@domain.com"), None);
        assert!(html.contains(r#"placeholder="you@domain.com""#),
            "must use override placeholder: {html}");
    }

    #[test]
    fn test_render_inline_subscribe_form_basic() {
        let html = render_hosted_subscribe_form(Some("test-site"), Language::En, "", None, None, "https://api.mosspub.com");
        assert!(html.contains(r#"<div class="moss-subscribe""#),
            "must wrap in moss-subscribe div: {html}");
        assert!(html.contains(r#"<form class="moss-subscribe-form" data-position="inline""#),
            "inner form must carry data-position=\"inline\": {html}");
        assert!(html.contains(r#"data-moss-hosted="true""#),
            "form must carry data-moss-hosted: {html}");
        assert!(html.contains(r#"action="https://api.mosspub.com/sites/test-site/subscribe""#),
            "form must POST to seta subscribe endpoint: {html}");
        // Description-paragraph rendering was removed when the unified
        // grammar moved description out of the shortcode and into prose.
        assert!(!html.contains("moss-subscribe-description"),
            "shortcode no longer emits a description paragraph: {html}");
    }

    #[test]
    fn test_hosted_form_wired_shape() {
        let html = render_hosted_subscribe_form(Some("site-abc"), Language::En, "en", None, None, "https://api.mosspub.com");
        assert!(html.contains(r#"<div class="moss-subscribe""#), "wraps in moss-subscribe div: {html}");
        assert!(html.contains(r#"<form class="moss-subscribe-form" data-position="inline" data-moss-hosted="true""#),
            "form is data-position=inline + moss-hosted: {html}");
        assert!(html.contains(r#"action="https://api.mosspub.com/sites/site-abc/subscribe""#),
            "wired action: {html}");
        assert!(!html.contains("data-moss-pending-site"), "wired form is not pending: {html}");
        assert!(!html.contains("data-button-override"), "no override marker without an override: {html}");
    }

    #[test]
    fn test_hosted_form_pending_shape() {
        let html = render_hosted_subscribe_form(None, Language::En, "", None, None, "https://api.mosspub.com");
        assert!(html.contains(r#"data-moss-pending-site="true""#), "None site_id → pending marker: {html}");
        assert!(html.contains(r##"action="#""##), "pending action is #: {html}");
    }

    #[test]
    fn test_hosted_form_empty_site_is_pending() {
        let html = render_hosted_subscribe_form(Some(""), Language::En, "", None, None, "https://api.mosspub.com");
        assert!(html.contains(r#"data-moss-pending-site="true""#), "empty site_id → pending: {html}");
        assert!(html.contains(r##"action="#""##), "empty site_id → pending action: {html}");
    }

    #[test]
    fn test_hosted_form_button_override_marker() {
        let with = render_hosted_subscribe_form(Some("s"), Language::En, "", None, Some("Request access"), "https://api.mosspub.com");
        assert!(with.contains(r#"data-button-override="true""#), "override present → marker: {with}");
        assert!(with.contains("Request access"), "override text rendered: {with}");
        let without = render_hosted_subscribe_form(Some("s"), Language::En, "", None, None, "https://api.mosspub.com");
        assert!(!without.contains("data-button-override"), "no override → no marker: {without}");
        let empty = render_hosted_subscribe_form(Some("s"), Language::En, "", None, Some(""), "https://api.mosspub.com");
        assert!(!empty.contains("data-button-override"), "empty override → no marker: {empty}");
    }

    #[test]
    fn test_hosted_form_escapes_scope() {
        let html = render_hosted_subscribe_form(Some("s"), Language::En, r#""><script>"#, None, None, "https://api.mosspub.com");
        assert!(!html.contains("<script>"), "scope must be html-escaped: {html}");
    }

    #[test]
    fn test_render_inline_subscribe_form_with_placeholder_override() {
        let html = render_hosted_subscribe_form(Some("test-site"),
            Language::En,
            "",
            Some("you@domain.com"),
            None,
            "https://api.mosspub.com",
        );
        assert!(html.contains(r#"placeholder="you@domain.com""#),
            "must use placeholder override: {html}");
    }

    #[test]
    fn test_render_inline_subscribe_form_with_button_override() {
        let html = render_hosted_subscribe_form(Some("test-site"), Language::En, "", None, Some("Request access"), "https://api.mosspub.com",
        );
        assert!(html.contains(">Request access<"));
    }

    #[test]
    fn test_render_inline_subscribe_form_html_escapes_overrides() {
        let html = render_hosted_subscribe_form(Some("test-site"), Language::En, "",
            Some("<script>alert(1)</script>"),
            Some("<b>Click</b>"),
            "https://api.mosspub.com",
        );
        assert!(!html.contains("<script>alert(1)</script>"));
        assert!(!html.contains("<b>Click</b>"));
        assert!(html.contains("&lt;script&gt;"));
    }

    #[test]
    fn test_render_inline_subscribe_form_zh_hans_default() {
        let html = render_hosted_subscribe_form(Some("test-site"), Language::ZhHans, "", None, None, "https://api.mosspub.com");
        assert!(html.contains("订阅"),
            "must use zh-hans default button label: {html}");
    }

    #[test]
    fn test_fetch_buttondown_username_bounded_on_unreachable() {
        // RFC 5737 TEST-NET-1 — guaranteed non-routable.
        let _env_guard = crate::ENV_TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let start = std::time::Instant::now();
        std::env::set_var("MOSS_BUTTONDOWN_API_BASE", "http://192.0.2.1");
        let result = fetch_buttondown_username("dummy-key");
        std::env::remove_var("MOSS_BUTTONDOWN_API_BASE");
        let elapsed = start.elapsed();
        assert!(result.is_none());
        assert!(
            elapsed < std::time::Duration::from_secs(5),
            "must fail fast (regression for #576): took {:?}", elapsed
        );
    }

}

#[cfg(test)]
mod scope_detection_tests {
    use super::*;

    #[test]
    fn empty_supported_scopes_always_returns_empty() {
        assert_eq!(detect_page_scope("/en/foo", &[]), "");
        assert_eq!(detect_page_scope("/foo", &[]), "");
    }

    #[test]
    fn matched_top_segment_returns_it() {
        let supported = vec!["en".to_string(), "zh".to_string()];
        assert_eq!(detect_page_scope("en/posts/a", &supported), "en");
        assert_eq!(detect_page_scope("/zh/posts/b", &supported), "zh");
    }

    #[test]
    fn unmatched_top_segment_returns_empty() {
        let supported = vec!["en".to_string()];
        assert_eq!(detect_page_scope("posts/a", &supported), "");
        assert_eq!(detect_page_scope("zh/posts/a", &supported), "");
    }

    #[test]
    fn root_path_returns_empty() {
        let supported = vec!["en".to_string()];
        assert_eq!(detect_page_scope("/", &supported), "");
        assert_eq!(detect_page_scope("index.html", &supported), "");
    }

    #[test]
    fn supported_scopes_excludes_non_language_top_folders() {
        // The single judge (lang_tree_prefix via derive_supported_scopes) keeps
        // only language-code top folders; section folders never mint a scope.
        let urls = [
            "en/index.html",
            "about/index.html",
            "zh/page.html",
            "posts/a.html",
            "studio-notes/b.html",
        ];
        let supported = derive_supported_scopes(urls.iter().copied());
        assert_eq!(supported, vec!["en".to_string(), "zh".to_string()]);
        // /about/index.html does NOT mint "about" as a scope.
        assert_eq!(detect_page_scope("about/index.html", &supported), "");
        assert_eq!(detect_page_scope("en/index.html", &supported), "en");
    }

    // ---- apply form emitter tests ----

    #[test]
    fn test_render_inline_apply_form_basic_zh_hans() {
        let html = render_inline_apply_form(
            "landing", Language::ZhHans, "", None, None, "https://api.mosspub.com",
        );
        assert!(
            html.contains(r#"class="moss-subscribe-form moss-apply-form""#),
            "must carry both form classes: {html}"
        );
        assert!(
            html.contains(r#"data-position="apply""#),
            "must have data-position=apply: {html}"
        );
        assert!(
            html.contains(r#"data-revert="false""#),
            "must opt out of auto-revert: {html}"
        );
        assert!(
            html.contains(r#"action="https://api.mosspub.com/apply?lang=zh-hans""#),
            "action must POST to seta /apply with lang: {html}"
        );
        assert!(html.contains(r#"name="matters""#), "must have matters field: {html}");
        assert!(html.contains(r#"name="website""#), "honeypot must be present: {html}");
        assert!(html.contains(r#"data-label-success=""#), "must have success label attr: {html}");
        assert!(html.contains("申请"), "must contain zh-hans submit label: {html}");
        // Redesigned form: two placeholder-only fields with a helper under each,
        // no visible <label> and no <details>/publish disclosure.
        assert!(
            html.contains("用于获取邀请及免费托管服务"),
            "zh-hans email helper must appear: {html}"
        );
        assert!(
            html.contains(r#"placeholder="Matters 用户名""#),
            "zh-hans second-field placeholder must appear: {html}"
        );
        assert!(
            html.contains(r#"maxlength="300""#),
            "second field caps at 300 — the all-Latin worst case of seta's weighted-300 matters cap: {html}"
        );
        assert!(
            html.contains("或者告诉我们你打算写什么，一句话就好"),
            "zh-hans second-field helper must appear: {html}"
        );
        assert!(
            html.contains(r#"class="moss-apply-helper""#),
            "helper element must carry moss-apply-helper class: {html}"
        );
        assert!(
            html.contains(r#"aria-label="邮箱""#) && html.contains(r#"aria-label="Matters 用户名""#),
            "inputs must be named for assistive tech via aria-label: {html}"
        );
        assert!(
            !html.contains(r#"name="publish""#),
            "publish field must be removed: {html}"
        );
        assert!(
            !html.contains("moss-apply-label") && !html.contains("moss-apply-details"),
            "visible labels and the details disclosure must be gone: {html}"
        );
    }

    #[test]
    fn test_render_inline_apply_form_zh_hant() {
        let html = render_inline_apply_form(
            "landing", Language::ZhHant, "", None, None, "https://api.mosspub.com",
        );
        assert!(
            html.contains(r#"action="https://api.mosspub.com/apply?lang=zh-hant""#),
            "must embed zh-hant lang param: {html}"
        );
        assert!(html.contains("申請"), "must use zh-hant label: {html}");
    }

    #[test]
    fn test_render_inline_apply_form_en() {
        let html = render_inline_apply_form(
            "landing", Language::En, "", None, None, "https://api.mosspub.com",
        );
        assert!(
            html.contains(r#"action="https://api.mosspub.com/apply?lang=en""#),
            "must embed en lang param: {html}"
        );
        assert!(html.contains("Apply"), "must use en label: {html}");
        assert!(
            html.contains(r#"placeholder="Matters username""#),
            "en second-field placeholder must not be Chinese: {html}"
        );
        assert!(
            html.contains("For your invitation and free hosting"),
            "en email helper must appear: {html}"
        );
        assert!(
            html.contains("Or tell us what you plan to write"),
            "en second-field helper must appear: {html}"
        );
        assert!(
            html.contains(r#"class="moss-apply-helper""#),
            "helper element must carry moss-apply-helper class: {html}"
        );
        assert!(
            !html.contains("moss-apply-label") && !html.contains(r#"name="publish""#),
            "visible labels and the publish field must be gone: {html}"
        );
    }

    #[test]
    fn test_render_inline_apply_form_html_escape_overrides() {
        let html = render_inline_apply_form(
            "landing",
            Language::En,
            "",
            Some("<script>xss</script>"),
            Some("<b>Click</b>"),
            "https://api.mosspub.com",
        );
        assert!(!html.contains("<script>xss</script>"), "placeholder must be escaped: {html}");
        assert!(!html.contains("<b>Click</b>"), "button override must be escaped: {html}");
        assert!(html.contains("&lt;script&gt;"), "escaped placeholder must appear: {html}");
    }

    #[test]
    fn test_render_inline_apply_form_no_site_id_in_action() {
        // The apply endpoint does NOT use the site_id in its URL (unlike subscribe).
        let html = render_inline_apply_form(
            "my-site-id", Language::En, "", None, None, "https://api.mosspub.com",
        );
        assert!(
            !html.contains("my-site-id"),
            "apply action must NOT embed site_id: {html}"
        );
    }
}

