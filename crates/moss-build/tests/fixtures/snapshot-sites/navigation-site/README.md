# navigation-site

Covers: docs/author/navigation.md

Exercises navigation features: nav bar (auto-populated from subfolders), nav ordering
via `weight`, explicit `nav: true` / `nav: false` overrides, `footer: true`,
`footer_align: right`, `breadcrumb: true` via cascade, and deeply nested pages.

Absorbed `test-sites/docs-with-nav/` (api/, changelog.md, contributing.md,
getting-started/, index.md). Supplemental frontmatter was added to exercise
all documented navigation features:

- `api/index.md` — `nav: true`, `weight: 1`, `cascade: breadcrumb: true`
- `getting-started/index.md` — `nav: true`, `weight: 1`, `cascade: breadcrumb: true`
- `contributing.md` — `nav: true`, `weight: 2`, `footer: true`, `footer_align: right`
- `changelog.md` — `nav: false`, `footer: true` (hidden from nav, shown in footer)
- A new `getting-started/index.md` was added (not in the original test-site).

## Layout

- `input/` — source site, as a user would author it.
- `expected/` — build output from `moss build input` with `--no-plugins`. Canonical
  snapshot; tests diff against this.

## Regenerating `expected/`

Run from the repo root:

```bash
cargo build -p moss-cli
./target/debug/moss-cli build crates/moss-build/tests/fixtures/snapshot-sites/navigation-site/input --no-plugins
# then diff .moss/build/staging vs expected/; if the diff is justified, replace expected/.
```

See `basic-site/README.md` for the regeneration procedure.
