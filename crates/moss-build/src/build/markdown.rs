//! Markdown parsing pipeline. Split per docs/archive/2026-04-24-codebase-restructure-continuation-plan.md Task 2.

pub mod body_plan;
pub mod frontmatter;
pub mod html_post;
pub mod math;
pub mod pipeline;
pub mod recent;

// Enumerate re-exports. Verified via:
// `grep -rn "build::markdown::" src-tauri/src/` and
// `grep -rn "super::markdown::\|use.*markdown::{" src-tauri/src/`

// ── frontmatter ────────────────────────────────────────────────────────
pub use frontmatter::{
    AnalyticsConfig,
    FrontMatter,
    compute_url_path,
    frontmatter_ref_to_stem,
    is_simplified_frontmatter,
    parse_simplified_frontmatter,
    parse_typed_frontmatter,
    // slug/uid re-exports forwarded from frontmatter (originally from generator::slug)
    generate_slug,
    generate_uid,
    insert_uid_into_frontmatter,
    replace_uid_in_frontmatter,
    resolve_duplicate_slugs_with_lang,
};

// ── pipeline ───────────────────────────────────────────────────────────
pub use pipeline::{
    process_markdown_file,
    render_markdown_to_html,
    render_markdown_to_html_with,
};

// ── html_post ──────────────────────────────────────────────────────────
pub use html_post::{
    adjust_relative_paths_for_pretty_urls,
    adjust_relative_paths_for_pretty_urls_with_overrides,
    apply_dir_overrides_to_asset_paths,
};
pub(crate) mod typed_renderers;
