//! Shortcode authoring catalog — the single source for "what shortcodes
//! exist, what attributes they take, and which of those name assets".
//!
//! Plainly: the editor's slash menu and fence autocomplete used to hand-copy
//! this knowledge in TypeScript (`SHORTCODE_CATALOG`, `ASSET_ATTR_BY_SHORTCODE`)
//! and the copies drifted (`apply` was missing). This table is generated into
//! `frontend/app/editor/shortcodes.generated.ts` by the `shortcode-catalog`
//! emitter in `src-tauri/dev-bin/generate-artifacts.rs`, CI-diff-gated like
//! `bindings.ts` — so the fact lives in Rust, once.
//!
//! Presentation (labels, hints, translations) is deliberately NOT here: hosts
//! overlay their own i18n on the structural catalog (design:
//! docs/archive/2026-08-11-cm6-extraction-design.md §4).
//!
//! The `entry()` match is total over [`ShortcodeKind`] — adding a variant
//! fails compilation HERE until the catalog describes it.

use crate::ast::shortcode::ShortcodeKind;
use crate::resolve::ext_kind::ExtKind;

/// One attribute a shortcode accepts on its opening fence line.
pub struct ShortcodeAttrSpec {
    /// Attribute name as written in `{name=…}`.
    pub name: &'static str,
    /// Non-empty when the attribute's VALUE names an asset file: editors
    /// scope asset search to these kinds. Empty for plain attrs
    /// (`cols=`, `button=`, …).
    pub asset_kinds: &'static [ExtKind],
}

/// The full authoring contract for one shortcode.
pub struct ShortcodeCatalogEntry {
    pub kind: ShortcodeKind,
    /// Fence name (`:::name`), equal to `kind.name()`.
    pub name: &'static str,
    pub attrs: &'static [ShortcodeAttrSpec],
    /// The opening-line attribute that names this shortcode's asset, if any
    /// (`hero` → `image`). `gallery` is deliberately absent: its assets are
    /// markdown embeds in the BODY, which editors already see as embeds.
    pub asset_attr: Option<&'static str>,
    /// Canonical English insertion template, CM6 snippet syntax
    /// (`${n:placeholder}` tab stops). Hosts may localise placeholders;
    /// the structure here is the one insertion grammar.
    pub canonical_template: &'static str,
    /// Whether editors offer this shortcode to authors — `kind.authorable()`.
    pub authorable: bool,
}

/// The catalog, in [`ShortcodeKind::all`] order (stable, deterministic).
pub fn catalog() -> Vec<ShortcodeCatalogEntry> {
    ShortcodeKind::all().map(entry).collect()
}

fn entry(kind: ShortcodeKind) -> ShortcodeCatalogEntry {
    let (attrs, asset_attr, canonical_template): (
        &'static [ShortcodeAttrSpec],
        Option<&'static str>,
        &'static str,
    ) = match kind {
        ShortcodeKind::Subscribe => (
            &[
                ShortcodeAttrSpec { name: "button", asset_kinds: &[] },
                ShortcodeAttrSpec { name: "placeholder", asset_kinds: &[] },
            ],
            None,
            "subscribe {button=\"${1:Subscribe}\"}\n:::",
        ),
        ShortcodeKind::Buttons => (
            &[],
            None,
            "buttons\n[${1:Get started}](${2:/})\n:::",
        ),
        ShortcodeKind::Gallery => (
            &[ShortcodeAttrSpec { name: "cols", asset_kinds: &[] }],
            None,
            "gallery {cols=${1:3}}\n![](${2:photo.jpg})\n:::",
        ),
        ShortcodeKind::Hero => (
            &[
                ShortcodeAttrSpec {
                    name: "image",
                    asset_kinds: &[ExtKind::Image, ExtKind::Video],
                },
                ShortcodeAttrSpec { name: "wide", asset_kinds: &[] },
            ],
            Some("image"),
            "hero {image=${1:photo.jpg}}\n# ${2:Title}\n${3:Subtitle}\n:::",
        ),
        ShortcodeKind::Grid => (
            &[
                ShortcodeAttrSpec { name: "cols", asset_kinds: &[] },
                ShortcodeAttrSpec { name: "wide", asset_kinds: &[] },
            ],
            None,
            "grid {cols=${1:2}}\n${2:cell one}\n+++\n${3:cell two}\n:::",
        ),
        ShortcodeKind::Recent => (
            &[
                ShortcodeAttrSpec { name: "count", asset_kinds: &[] },
                ShortcodeAttrSpec { name: "since", asset_kinds: &[] },
                ShortcodeAttrSpec { name: "last", asset_kinds: &[] },
            ],
            None,
            "recent {count=${1:5}}\n${2:No posts yet.}\n:::",
        ),
        ShortcodeKind::Apply => (
            &[
                ShortcodeAttrSpec { name: "placeholder", asset_kinds: &[] },
                ShortcodeAttrSpec { name: "button", asset_kinds: &[] },
            ],
            None,
            "apply {button=\"${1:Apply}\"}\n:::",
        ),
    };
    ShortcodeCatalogEntry {
        kind,
        name: kind.name(),
        attrs,
        asset_attr,
        canonical_template,
        authorable: kind.authorable(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn covers_every_kind_exactly_once() {
        let cat = catalog();
        assert_eq!(cat.len(), ShortcodeKind::all().count());
        let names: std::collections::HashSet<_> = cat.iter().map(|e| e.name).collect();
        assert_eq!(names.len(), cat.len(), "duplicate fence names");
    }

    #[test]
    fn name_matches_serde_snake_case() {
        for e in catalog() {
            let serde_name = serde_json::to_string(&e.kind).expect("serialize");
            assert_eq!(serde_name, format!("\"{}\"", e.name));
        }
    }

    #[test]
    fn asset_attr_names_a_declared_asset_attr() {
        for e in catalog() {
            if let Some(attr) = e.asset_attr {
                let hit = e.attrs.iter().find(|a| a.name == attr);
                let hit = hit.unwrap_or_else(|| {
                    panic!("{}: asset_attr `{attr}` is not in attrs", e.name)
                });
                assert!(
                    !hit.asset_kinds.is_empty(),
                    "{}: asset_attr `{attr}` declares no asset kinds",
                    e.name
                );
            }
            // And the inverse: an opening-line attr with asset kinds must be
            // reachable — i.e. be THE asset_attr — or editors could never
            // search assets for it.
            for a in e.attrs.iter().filter(|a| !a.asset_kinds.is_empty()) {
                assert_eq!(
                    e.asset_attr,
                    Some(a.name),
                    "{}: attr `{}` carries asset kinds but is not the asset_attr",
                    e.name,
                    a.name
                );
            }
        }
    }

    #[test]
    fn template_opens_with_the_fence_name_and_closes_the_fence() {
        for e in catalog() {
            assert!(
                e.canonical_template.starts_with(e.name),
                "{}: template must start with the fence name (inserted after `:::`)",
                e.name
            );
            assert!(
                e.canonical_template.ends_with(":::"),
                "{}: template must close its fence",
                e.name
            );
        }
    }

    #[test]
    fn only_apply_is_hidden() {
        for e in catalog() {
            assert_eq!(
                e.authorable,
                e.kind != ShortcodeKind::Apply,
                "{}: authorable flag drifted from the §7.1 decision",
                e.name
            );
        }
    }
}
