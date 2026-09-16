// `ComponentEntry` and `Status` are imported to verify they are part of the
// public API surface of the module, even though they're only used through
// field/value access on `COMPONENTS` entries below.
#[allow(unused_imports)]
use moss_core::contract::components::{
    is_unprefixed_legacy, ComponentEntry, Status, COMPONENTS, UNPREFIXED_LEGACY_CLASSES,
};

#[test]
fn is_public_returns_false_for_retired_entries() {
    // moss-cards-grid is Retired — is_public() must return false.
    let retired = COMPONENTS
        .iter()
        .find(|e| e.class == "moss-cards-grid")
        .expect("moss-cards-grid must be in COMPONENTS");
    assert!(
        !retired.is_public(),
        "retired entry 'moss-cards-grid' must not be is_public()"
    );
}

#[test]
fn is_public_returns_true_for_confirmed_entries() {
    // moss-cards is Confirmed — is_public() must return true.
    let confirmed = COMPONENTS
        .iter()
        .find(|e| e.class == "moss-cards")
        .expect("moss-cards must be in COMPONENTS");
    assert!(
        confirmed.is_public(),
        "confirmed entry 'moss-cards' must be is_public()"
    );
}

#[test]
fn components_table_is_non_empty() {
    assert!(!COMPONENTS.is_empty(), "COMPONENTS must contain at least one entry");
}

#[test]
fn every_component_has_a_class_name() {
    // The exemption list lives beside the table in `components.rs`, not here —
    // an unprefixed class is a decision made when the entry is written, and it
    // has to be visible to whoever writes it. See `UNPREFIXED_LEGACY_CLASSES`.
    for entry in COMPONENTS {
        assert!(
            entry.class.starts_with("moss-") || is_unprefixed_legacy(entry.class),
            "class '{}' must be moss-prefixed, or be added to \
             UNPREFIXED_LEGACY_CLASSES with a reason",
            entry.class
        );
    }
}

/// The exemption list must not outlive the entries it exempts.
///
/// Without this, removing an unprefixed class from `COMPONENTS` leaves a name
/// in `UNPREFIXED_LEGACY_CLASSES` that silently pre-authorizes re-adding it
/// unprefixed later — the allowlist would only ever grow, and would stop
/// describing what moss actually emits.
#[test]
fn no_exemption_outlives_its_entry() {
    for exempt in UNPREFIXED_LEGACY_CLASSES {
        assert!(
            COMPONENTS.iter().any(|e| e.class == *exempt),
            "'{exempt}' is exempted from the moss- prefix rule but is no longer \
             in COMPONENTS — drop it from UNPREFIXED_LEGACY_CLASSES"
        );
    }
}

#[test]
fn components_table_has_no_duplicate_classes() {
    use std::collections::HashSet;
    let mut seen = HashSet::new();
    for entry in COMPONENTS {
        assert!(
            seen.insert(entry.class),
            "duplicate class in COMPONENTS: {}",
            entry.class
        );
    }
}

#[test]
fn moss_cards_entry_has_expected_shape() {
    let cards = COMPONENTS.iter().find(|e| e.class == "moss-cards")
        .expect("moss-cards must be in COMPONENTS");
    assert_eq!(cards.kind, "container");
    assert!(cards.data_attrs.iter().any(|a| a.name == "data-layout"));
}

#[test]
fn every_authorable_shortcode_has_nonempty_example_markdown() {
    use moss_core::ast::shortcode::ShortcodeKind;
    for kind in ShortcodeKind::all() {
        let cls = kind.root_class();
        let e = COMPONENTS
            .iter()
            .find(|e| e.class == cls)
            .unwrap_or_else(|| panic!("authorable class {cls} missing from COMPONENTS"));
        assert!(
            !e.example_markdown.is_empty(),
            "authorable shortcode {cls} needs example_markdown"
        );
    }
}

#[test]
fn authorable_example_markdown_renders_its_class() {
    use moss_core::ast::{parse, render_document, DefaultHooks, ResolvedUrl, Url, UrlKind};
    use moss_core::ast::shortcode::ShortcodeKind;
    use moss_core::ast::visit_urls_mut;
    for kind in ShortcodeKind::all() {
        let cls = kind.root_class();
        let md = COMPONENTS
            .iter()
            .find(|e| e.class == cls)
            .unwrap()
            .example_markdown;
        let mut doc = parse(md);
        // Resolve all Unresolved URLs to a trivial external href so
        // shortcodes that contain links or images (buttons, gallery) do not
        // hit the debug_assert for Unresolved URLs in DefaultHooks.
        visit_urls_mut(&mut doc, |url| {
            if matches!(url, Url::Unresolved(_)) {
                *url = Url::Resolved(ResolvedUrl {
                    href: "https://example.com/placeholder".to_string(),
                    kind: UrlKind::External,
                });
            }
        });
        let html = render_document(&doc, &DefaultHooks::new());
        assert!(
            html.contains(cls),
            "rendering {cls} example_markdown must emit class {cls}; got:\n{html}"
        );
    }
}

/// Drift gate (arch-review #776): `ShortcodeKind::all()` is a hand-maintained
/// array, the SSOT for "which shortcodes are authorable". This test makes a
/// new enum variant impossible to add silently: the exhaustive `match` below
/// fails to COMPILE until the new variant is handled, and the count/coverage
/// assertions then fail until `all()` lists it — so a new shortcode can't skip
/// the authorable flag, example_markdown enforcement, or the render round-trip.
#[test]
fn shortcode_kind_all_enumerates_every_variant() {
    use moss_core::ast::shortcode::ShortcodeKind;
    let all: Vec<ShortcodeKind> = ShortcodeKind::all().collect();
    // Compile-time exhaustiveness: adding a variant breaks this match.
    for k in &all {
        match k {
            ShortcodeKind::Subscribe
            | ShortcodeKind::Buttons
            | ShortcodeKind::Gallery
            | ShortcodeKind::Hero
            | ShortcodeKind::Grid
            | ShortcodeKind::Recent
            | ShortcodeKind::Apply => {}
        }
    }
    let expected = [
        ShortcodeKind::Subscribe,
        ShortcodeKind::Buttons,
        ShortcodeKind::Gallery,
        ShortcodeKind::Hero,
        ShortcodeKind::Grid,
        ShortcodeKind::Recent,
        ShortcodeKind::Apply,
    ];
    assert_eq!(all.len(), expected.len(), "ShortcodeKind::all() must list every variant");
    for e in expected {
        assert!(all.contains(&e), "ShortcodeKind::all() is missing {e:?}");
        assert!(
            COMPONENTS.iter().any(|c| c.class == e.root_class()),
            "authorable shortcode {:?} maps to {} which is not in COMPONENTS",
            e, e.root_class()
        );
    }
}
