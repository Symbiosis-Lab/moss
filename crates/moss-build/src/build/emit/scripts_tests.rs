use super::*;
use std::collections::BTreeSet;

fn all_on() -> SiteAssets {
    SiteAssets {
        callouts: true,
        link_preview: true,
        heading_anchors: true,
        math: true,
        media_pages: true,
        search: true,
        vertical: true,
        has_footnotes: true,
        scroll_rows: true,
        video_ladder: true,
    }
}

/// Totality: every `assets/js/*.js` that a site could serve has a row.
///
/// A script with no row is never emitted, and the failure surfaces as a 404 in
/// a reader's console — not in any build. The exclusions are the bundles that
/// are inlined into HTML or injected by the preview server, which never become
/// files under `_moss/js/`.
#[test]
fn every_embedded_script_has_a_row() {
    // Embedded by this crate but inlined into HTML or injected by the preview
    // server rather than emitted as a file under `_moss/js/`.
    const NOT_EMITTED: &[&str] = &[
        "comments-artalk", // inlined by features::comment::render
        "subscribe",       // inlined by features (email::SUBSCRIBE_JS)
    ];
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/assets/js");
    let in_table: BTreeSet<&str> = SITE_SCRIPTS.iter().map(|s| s.name).collect();
    let mut unregistered = Vec::new();
    for entry in std::fs::read_dir(&dir).expect("assets/js must exist") {
        let path = entry.expect("readable dir entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("js") {
            continue;
        }
        let stem = path.file_stem().unwrap().to_str().unwrap().to_string();
        if NOT_EMITTED.contains(&stem.as_str()) || in_table.contains(stem.as_str()) {
            continue;
        }
        unregistered.push(stem);
    }
    assert!(
        unregistered.is_empty(),
        "scripts with no SITE_SCRIPTS row (they will never be emitted): {unregistered:?}. \
         If one is inlined or preview-only, add it to NOT_EMITTED with a reason."
    );
}

/// hls.js is stored deflate-compressed (`flate!`) rather than `include_str!`-ed
/// raw; the inflated bytes must be exactly the file on disk, or every emitted
/// copy and its content hash silently change.
#[test]
fn compressed_hls_inflates_to_the_source_file() {
    let hls = SITE_SCRIPTS.iter().find(|s| s.name == "hls").unwrap();
    let on_disk = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(hls.dev_path),
    )
    .expect("read src/assets/js/hls.js");
    assert_eq!((hls.source)(), on_disk, "inflated hls.js differs from the source file");
}

/// Every row must name a file that exists and is non-empty. An empty
/// `include_str!` compiles fine and ships a script that does nothing.
#[test]
fn every_row_embeds_real_bytes() {
    for script in SITE_SCRIPTS {
        assert!(
            !(script.source)().trim().is_empty(),
            "script '{}' embeds nothing — is dev_path/source pointing at the right file?",
            script.name
        );
        assert!(
            script.dev_path.ends_with(&format!("{}.js", script.name)),
            "script '{}' dev_path {:?} does not name it — the dev-mode read would load a \
             different file than the release-mode embed, so hashes would differ by build mode",
            script.name,
            script.dev_path
        );
    }
}

/// Gates must actually gate. If every gate were `|_| true` this module would
/// be a rename of the old code with none of the point.
#[test]
fn gates_change_which_scripts_ship() {
    let none = SiteAssets::default();
    let shipped = |a: &SiteAssets| -> BTreeSet<&str> {
        SITE_SCRIPTS.iter().filter(|s| (s.gate)(a)).map(|s| s.name).collect()
    };
    let off = shipped(&none);
    let on = shipped(&all_on());
    assert!(off.len() < on.len(), "an all-false asset set must ship fewer scripts");
    // The unconditional two are the floor. (The blueprint placeholder ships on
    // every previewed page, but the preview server injects it — no row here,
    // and nothing in a published site.)
    for always in ["theme", "share-card"] {
        assert!(off.contains(always), "'{always}' must ship regardless of config");
    }
    // And the five gated ones are genuinely absent.
    for gated in ["preview", "heading-anchor", "math-copy", "search", "fullscreen"] {
        assert!(!off.contains(gated), "'{gated}' must not ship when its gate is off");
    }
}

/// Each gate reads its OWN field. A copy-paste that pointed two rows at one
/// field would make a config flag silently control the wrong script.
#[test]
fn each_gate_reads_a_distinct_fact() {
    let fields: &[(&str, fn(&mut SiteAssets))] = &[
        ("preview", |a| a.link_preview = true),
        ("heading-anchor", |a| a.heading_anchors = true),
        ("math-copy", |a| a.math = true),
        ("search", |a| a.search = true),
        ("fullscreen", |a| a.media_pages = true),
    ];
    for (name, set) in fields {
        let mut assets = SiteAssets::default();
        set(&mut assets);
        let on: BTreeSet<&str> = SITE_SCRIPTS
            .iter()
            .filter(|s| (s.gate)(&assets))
            .map(|s| s.name)
            .collect();
        let gated: BTreeSet<&str> = on
            .iter()
            .copied()
            .filter(|n| !["theme", "share-card"].contains(n))
            .collect();
        assert_eq!(
            gated,
            BTreeSet::from([*name]),
            "setting the fact for '{name}' turned on {gated:?}"
        );
    }
}

/// Names must be unique and URL-safe: they become the file stem.
#[test]
fn names_are_unique_and_valid_served_paths() {
    let mut seen = BTreeSet::new();
    for script in SITE_SCRIPTS {
        assert!(seen.insert(script.name), "duplicate script name '{}'", script.name);
        assert!(
            ServedPath::for_runtime_js_hashed(script.name, "0123456789abcdef").is_ok(),
            "'{}' is not a valid runtime JS name",
            script.name
        );
    }
}

/// Every lazy chunk must be an ES module — an `iife` build exports nothing, so
/// the `import()` that fetches it resolves to an empty namespace and whatever
/// it destructures is `undefined`. The symptoms are silent: a Share button that
/// does nothing, a video that never upgrades off the progressive MP4.
///
/// This asserted `lazy.len() == 1` until `hls` became the second chunk. The
/// count was a proxy for the real invariant and only ever fired on adding one.
#[test]
fn every_lazy_chunk_is_an_es_module() {
    let lazy: Vec<&SiteScript> = SITE_SCRIPTS.iter().filter(|s| s.load == Load::Lazy).collect();
    assert!(!lazy.is_empty(), "the lazy-chunk mechanism has no users left");
    for script in lazy {
        assert!(
            (script.source)().contains("export{") || (script.source)().contains("export {"),
            "{}.js must be built with format: \"esm\" in build-backend-scripts.mjs — an iife \
             bundle has no export for the dynamic import to destructure, and the feature \
             fails silently",
            script.name
        );
    }
}

/// Conversely, an eager bundle must NOT be an ES module: it is loaded with a
/// classic `<script src>`, where a bare `export` is a syntax error that kills
/// the whole file.
#[test]
fn eager_bundles_are_not_es_modules() {
    for script in SITE_SCRIPTS.iter().filter(|s| s.load != Load::Lazy) {
        // A positive check on the last non-empty line, not a disjunction of
        // negatives: `!ends_with(…) || !contains(…)` passes vacuously the
        // moment esbuild emits `export {a};` with a space, which is exactly
        // when this guard would be needed.
        let last = (script.source)().lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("");
        assert!(
            !last.trim_start().starts_with("export"),
            "'{}' ends with `{last}` — an ES module loaded by a classic <script src>, \
             where a bare `export` is a syntax error that kills the whole file",
            script.name
        );
    }
}

/// The point of Milestone B: share-card's weight left theme.js.
///
/// Pinned as a ratio rather than a byte count so it survives ordinary code
/// growth but fails if a static `import` re-links the chunk into the parent —
/// which is exactly how it got there, via one `cleanBlockText` import.
#[test]
fn share_card_is_not_bundled_into_theme() {
    let theme = SITE_SCRIPTS.iter().find(|s| s.name == "theme").unwrap();
    let share = SITE_SCRIPTS.iter().find(|s| s.name == "share-card").unwrap();
    assert!(
        !(theme.source)().contains("drawJustifiedWrappedLine"),
        "theme.js contains the share-card text engine — something re-linked it statically"
    );
    assert!(
        (theme.source)().len() < (share.source)().len() * 3,
        "theme.js is {} bytes against share-card's {}; the split has regressed",
        (theme.source)().len(),
        (share.source)().len()
    );
}

/// `all_hashes` must cover the table. It is the input to
/// `FacadeCache::asset_versions`, and a script missing from it is a script
/// whose filename can move without invalidating carried-forward pages.
#[test]
fn all_hashes_covers_every_row() {
    let resolved = ScriptAssets::resolve();
    assert_eq!(resolved.all_hashes().len(), SITE_SCRIPTS.len());
    for script in SITE_SCRIPTS {
        let h = resolved.hash(script.name);
        assert_eq!(h.len(), 16, "'{}' hash is not 16 hex chars", script.name);
    }
}

/// Distinct scripts must hash distinctly — otherwise two would collide on one
/// filename and one would overwrite the other.
#[test]
fn every_script_hashes_distinctly() {
    let resolved = ScriptAssets::resolve();
    let hashes: BTreeSet<&str> = resolved.all_hashes().into_iter().collect();
    assert_eq!(hashes.len(), SITE_SCRIPTS.len(), "two scripts share a content hash");
}

#[test]
#[should_panic(expected = "no script named")]
fn asking_for_an_unregistered_script_panics() {
    let _ = ScriptAssets::resolve().hash("not-a-real-script");
}

/// The shell block is the table, in order, with the table's `defer`.
///
/// These are classic `<script src>` tags, so they execute in the order the
/// block emits them, and that order is now the table's — moving a row moves
/// the tag. This is what notices such a move; the golden fixtures under
/// `tests/fixtures/*/expected/` pin the bytes for the configurations they
/// happen to cover, which is not every row.
#[test]
fn shell_tags_follow_the_table_in_order() {
    let resolver = crate::build::assets::paths::PathResolver::new();
    let block = ScriptAssets::resolve().shell_tags(&all_on(), &resolver);
    let names: Vec<&str> = block
        .lines()
        .filter(|l| l.contains("<script"))
        .map(|l| l.split("/_moss/js/").nth(1).unwrap().split('.').next().unwrap())
        .collect();
    let expected: Vec<&str> = SITE_SCRIPTS
        .iter()
        .filter(|s| matches!(s.load, Load::Shell { .. }))
        .map(|s| s.name)
        .collect();
    assert_eq!(names, expected, "the shell block must be SITE_SCRIPTS order");
    // `theme` and `fullscreen` are eager but placed by their own templates,
    // and the lazy chunk has no tag at all — none of the three may leak here.
    for placed in ["theme", "fullscreen", "share-card"] {
        assert!(!names.contains(&placed), "'{placed}' is not placed by the shell block");
    }
    assert!(
        block.contains("js/search.") && block.lines().filter(|l| l.contains(" defer")).count() == 1,
        "search is the one row carrying defer, and it must reach the tag"
    );
}

/// A gate that is off contributes NO tag — not an empty one. With the markup
/// it acts on suppressed, the bundle is not emitted either, so a tag would
/// 404 on every page load.
#[test]
fn shell_tags_omit_what_the_gates_turn_off() {
    let resolver = crate::build::assets::paths::PathResolver::new();
    let scripts = ScriptAssets::resolve();
    assert_eq!(
        scripts.shell_tags(&SiteAssets::default(), &resolver),
        "",
        "an all-false asset set must produce an empty block, not empty tags"
    );
    let one = SiteAssets { math: true, ..SiteAssets::default() };
    let block = scripts.shell_tags(&one, &resolver);
    assert_eq!(block.matches("<script").count(), 1, "one fact on, one tag: {block}");
    assert!(block.contains("/_moss/js/math-copy."), "and it must be math-copy: {block}");
}

/// The lazy chunk has no `<script src>` to ask for. `tag` refusing is what
/// keeps that true from the OTHER direction than `shell_tags`' filter: a
/// template placing its own tag reaches `tag` by name, where a typo or a
/// later `Load` change would otherwise emit a classic tag for an ES module
/// and take the page down with a syntax error.
#[test]
#[should_panic(expected = "lazy chunk")]
fn asking_for_a_tag_on_the_lazy_chunk_panics() {
    let resolver = crate::build::assets::paths::PathResolver::new();
    let _ = ScriptAssets::resolve().tag("share-card", &all_on(), &resolver);
}
