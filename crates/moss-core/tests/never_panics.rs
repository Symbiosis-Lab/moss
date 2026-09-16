//! Property tests that pin moss-core's panic-free contract.
//!
//! moss-core is invoked from Tauri command handlers in a host process
//! configured `panic = "abort"` for release. A panic on user input is a
//! production crash. The bug fixed in `date.rs` (commit 98dfe7419) was
//! exactly this shape: 1 line of byte-indexed `&str` slicing aborted the
//! whole desktop app on a Chinese filename.
//!
//! These tests fuzz every public string-consuming function in moss-core
//! with arbitrary UTF-8 input and assert the call doesn't panic. They
//! don't validate output correctness (existing unit tests cover that) —
//! they just enumerate the panic-free contract so a future regression
//! surfaces in CI rather than at user runtime.
//!
//! Adding a new public string-consuming function? Add a proptest here.
//! It's the cheapest insurance against a 5-minute coding mistake
//! aborting every user's session.

use proptest::option;
use proptest::prelude::*;
use std::collections::HashMap;

// ── tuning ──────────────────────────────────────────────────────────────────
//
// 256 cases per function × ~20 functions ≈ 5k test invocations. Proptest's
// shrinker amortises the bulk of the work into the first dozen failures,
// so 256 is plenty to surface any regression of the byte-slicing /
// unwrap-on-user-input bug class. Bumping higher costs CI time and gains
// little.
fn cfg() -> ProptestConfig {
    ProptestConfig::with_cases(256)
}

// Helper: arbitrary UTF-8 string with bounded length so individual cases
// don't drag in pathological 100KB inputs that slow CI without adding
// signal.
fn any_str() -> impl Strategy<Value = String> {
    ".{0,256}".prop_map(String::from)
}

// ── frontmatter ─────────────────────────────────────────────────────────────

proptest! {
    #![proptest_config(cfg())]

    #[test]
    fn frontmatter_parse_never_panics(content in any_str()) {
        let _ = moss_core::frontmatter::parse(&content);
    }

    #[test]
    fn frontmatter_serialize_never_panics(
        keys in proptest::collection::vec(any_str(), 0..8),
        values in proptest::collection::vec(any_str(), 0..8),
        body in any_str(),
    ) {
        let mut fm = HashMap::new();
        for (k, v) in keys.into_iter().zip(values.into_iter()) {
            fm.insert(k, serde_yaml::Value::String(v));
        }
        let _ = moss_core::frontmatter::serialize(&fm, &body);
    }
}

// ── slug generation ─────────────────────────────────────────────────────────

proptest! {
    #![proptest_config(cfg())]

    #[test]
    fn generate_slug_never_panics(path in any_str()) {
        let _ = moss_core::content_graph::generate_slug(&path);
    }
}

// ── date resolution (already had a proptest in date.rs; pin again here) ─────

proptest! {
    #![proptest_config(cfg())]

    #[test]
    fn resolve_publish_date_never_panics(
        name in any_str(),
        ctime in option::of(any_str()),
    ) {
        let _ = moss_core::date::resolve_publish_date(
            &HashMap::new(),
            &[name.as_str()],
            ctime.as_deref(),
        );
    }
}

// ── home / language detection ───────────────────────────────────────────────

proptest! {
    #![proptest_config(cfg())]

    #[test]
    fn lang_tree_prefix_never_panics(path in any_str()) {
        let _ = moss_core::home::lang_tree_prefix(&path);
    }

    #[test]
    fn strip_lang_suffix_never_panics(stem in any_str()) {
        let _ = moss_core::home::strip_lang_suffix(&stem);
    }

    #[test]
    fn is_index_stem_never_panics(stem in any_str()) {
        let _ = moss_core::home::is_index_stem(&stem);
    }

    #[test]
    fn is_home_file_never_panics(stem in any_str(), parent in any_str()) {
        let _ = moss_core::home::is_home_file(&stem, &parent);
    }

    #[test]
    fn detect_home_file_never_panics(filenames in proptest::collection::vec(any_str(), 0..12)) {
        let refs: Vec<&str> = filenames.iter().map(|s| s.as_str()).collect();
        let _ = moss_core::home::detect_home_file(&refs);
    }
}

// ── media wikilink parsing ──────────────────────────────────────────────────

proptest! {
    #![proptest_config(cfg())]

    #[test]
    fn strip_wikilink_never_panics(raw in any_str()) {
        let _ = moss_core::media::strip_wikilink(&raw);
    }

    #[test]
    fn split_pipe_never_panics(raw in any_str()) {
        let _ = moss_core::media::split_pipe(&raw);
    }

    #[test]
    fn parse_media_attrs_never_panics(raw in any_str()) {
        let _ = moss_core::media::parse_media_attrs(&raw);
    }

    #[test]
    fn is_all_display_keywords_never_panics(text in any_str()) {
        let _ = moss_core::media::is_all_display_keywords(&text);
    }

    #[test]
    fn html_escape_never_panics(s in any_str()) {
        let _ = moss_core::media::html_escape(&s);
    }
}

// ── link candidates / heading anchors ───────────────────────────────────────

proptest! {
    #![proptest_config(cfg())]

    #[test]
    fn link_candidates_never_panics(target in any_str()) {
        let _ = moss_core::link_candidates::link_candidates(&target);
    }

    #[test]
    fn obsidian_heading_anchor_never_panics(heading in any_str()) {
        let _ = moss_core::heading::anchor::obsidian_heading_anchor(&heading);
    }

    #[test]
    fn filename_text_never_panics(path in any_str()) {
        let _ = moss_core::heading::filename_text(&path);
    }
}

// ── markdown body transforms ────────────────────────────────────────────────

proptest! {
    #![proptest_config(cfg())]

    // Phase 4 PR4 (2026-05-27): `transform_callouts_never_panics` test
    // removed alongside deletion of `crates/moss-core/src/resolve/callouts.rs`.
    // Obsidian-callout detection is now part of the typed AST parser
    // (`crates/moss-core/src/ast/parser.rs`'s Tag::BlockQuote arm); the
    // existing `parse_never_panics` proptest below already covers it.

    #[test]
    fn transform_block_refs_never_panics(content in any_str()) {
        let _ = moss_core::resolve::block_refs::transform_block_refs(&content);
    }
}

// ── AST / shortcode extraction ──────────────────────────────────────────────

proptest! {
    #![proptest_config(cfg())]

    #[test]
    fn ast_parse_never_panics(markdown in any_str()) {
        let _ = moss_core::ast::parser::parse(&markdown);
    }

    /// `any_str()` almost never produces `[![[`, so the linked-embed
    /// substitution (`ast::linked_embed`, byte-index arithmetic over a masked
    /// copy) would be untested by `ast_parse_never_panics` above. This draws
    /// from the alphabet that pass actually scans — brackets, `!`, parens, the
    /// pothole `|`, the escape `\`, a newline, and a multi-byte char to catch
    /// any offset that lands mid-UTF-8.
    #[test]
    fn ast_parse_never_panics_on_linked_embed_soup(
        markdown in r"([\[\]!()|\\\n ]|x|好){0,64}".prop_map(String::from)
    ) {
        let _ = moss_core::ast::parser::parse(&markdown);
    }

    #[test]
    fn extract_shortcodes_never_panics(markdown in any_str()) {
        let _ = moss_core::ast::shortcode_extract::extract_shortcodes(&markdown);
    }

    #[test]
    fn split_cells_never_panics(body in any_str()) {
        let _ = moss_core::ast::cells::split_cells(&body);
    }

    #[test]
    fn parse_attrs_never_panics(input in any_str()) {
        let _ = moss_core::ast::attrs::parse_attrs(&input);
    }
}

// ── schema parsing ──────────────────────────────────────────────────────────

proptest! {
    #![proptest_config(cfg())]

    #[test]
    fn parse_schema_never_panics(json in any_str()) {
        let _ = moss_core::schema::parse_schema(&json);
    }
}

// ── csv / table rendering ───────────────────────────────────────────────────

proptest! {
    #![proptest_config(cfg())]

    #[test]
    fn csv_table_render_never_panics(content in any_str(), separator in prop::char::any()) {
        let opts = moss_core::csv_table::CsvTableOptions {
            separator,
            has_header: true,
            caption: None,
            class: "moss-table".to_string(),
            data_type: None,
        };
        let _ = moss_core::csv_table::render(&content, &opts);
    }
}

// ── heading: hero detection ─────────────────────────────────────────────────

proptest! {
    #![proptest_config(cfg())]

    #[test]
    fn body_starts_with_hero_never_panics(body in any_str()) {
        let _ = moss_core::heading::hero_at_top_owns_title(&body);
    }
}

// ── shortcode tokens ────────────────────────────────────────────────────────
//
// The tokenize family walks a line byte-by-byte against an ASCII grammar
// (`:`, `{`, `}`, `.`, identifier chars). Exactly the byte-vs-char territory
// the original `date.rs` panic lived in, so it deserves explicit fuzz coverage.

proptest! {
    #![proptest_config(cfg())]

    #[test]
    fn tokenize_opening_line_never_panics(line in any_str()) {
        let _ = moss_core::shortcode_tokens::tokenize_opening_line(&line);
    }

    #[test]
    fn tokenize_closing_line_never_panics(line in any_str()) {
        let _ = moss_core::shortcode_tokens::tokenize_closing_line(&line);
    }

    #[test]
    fn tokenize_divider_line_never_panics(line in any_str()) {
        let _ = moss_core::shortcode_tokens::tokenize_divider_line(&line);
    }

    #[test]
    fn tokens_to_html_never_panics(line in any_str()) {
        // Tokens are derived from the same line; pair the two so the HTML
        // renderer sees realistically-shaped offsets rather than random ones
        // that could violate a precondition the public API doesn't promise.
        let tokens = moss_core::shortcode_tokens::tokenize_opening_line(&line);
        let _ = moss_core::shortcode_tokens::tokens_to_html(&line, &tokens);
    }
}

// ── fuzzy path / URL encoding ───────────────────────────────────────────────
//
// fuzzy_path manipulates URL/path strings with byte offsets pulled from
// `find('/')` and `find('.')`. Several call sites carry an audited
// `#[allow(clippy::string_slice)]` for the byte-arithmetic; these proptests
// pin the audit at the public-API surface.

proptest! {
    #![proptest_config(cfg())]

    #[test]
    fn fuzzy_relative_url_never_panics(from in any_str(), to in any_str()) {
        let _ = moss_core::resolve::fuzzy_path::relative_url(&from, &to);
    }

    #[test]
    fn fuzzy_relative_asset_path_never_panics(from in any_str(), to in any_str()) {
        let _ = moss_core::resolve::fuzzy_path::relative_asset_path(&from, &to);
    }

    #[test]
    fn fuzzy_percent_encode_path_segments_never_panics(path in any_str()) {
        let _ = moss_core::resolve::fuzzy_path::percent_encode_path_segments(&path);
    }

    #[test]
    fn fuzzy_split_url_path_never_panics(url in any_str()) {
        let _ = moss_core::resolve::fuzzy_path::split_url_path(&url);
    }

    #[test]
    fn fuzzy_percent_encode_url_never_panics(url in any_str()) {
        let _ = moss_core::resolve::fuzzy_path::percent_encode_url(&url);
    }
}

// ── shortcode placeholder round-trip ────────────────────────────────────────

proptest! {
    #![proptest_config(cfg())]

    #[test]
    fn placeholder_for_never_panics(nonce in any_str(), index in 0usize..=1024) {
        let _ = moss_core::ast::shortcode_extract::placeholder_for(&nonce, index);
    }

    #[test]
    fn parse_placeholder_never_panics(nonce in any_str(), html in any_str()) {
        let _ = moss_core::ast::shortcode_extract::parse_placeholder(&nonce, &html);
    }
}

// ── attrs: brace depth tracking ─────────────────────────────────────────────

proptest! {
    #![proptest_config(cfg())]

    #[test]
    fn brace_depth_never_panics(s in any_str(), start_depth in -32i32..=32) {
        let _ = moss_core::ast::attrs::brace_depth(&s, start_depth);
    }
}
