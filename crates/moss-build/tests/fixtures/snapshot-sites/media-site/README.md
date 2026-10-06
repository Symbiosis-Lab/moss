# media-site

Covers: docs/media.md

Exercises the media pipeline (images, video, asset copies, nested media).

## Layout

This fixture is structured as a parent with sub-fixtures:

- `input/` + `expected/` — top-level media-site, covers `docs/media.md` examples.
- `pipeline-assets/input/` — asset pipeline edge cases (used by `pipeline_correctness.rs` `test_asset_pipeline`).
- `pipeline-nested/input/` — nested directory behavior (used by `test_nested_directory_structure` and `test_immersive_page_types`).
- `image-pages/input/` — image-specific page rendering.

Tests that consume the sub-fixtures call `setup_fixture("media-site/<name>")`.

See `basic-site/README.md` for the regeneration procedure.

## Note on `expected/llms.txt`

`llms.txt` is built from `ParsedDocument::content`, which is the post-frontmatter body straight from `gray_matter`'s splitter. Phase 3 PR2 retired the Stage 1 wikilink rewriter (commit `ee102b7d1`), so image-form wikilinks like `![[logo.png]]` and plain `[[images]]` now reach this output unchanged — Stage 2's wikilink dispatcher operates inside pulldown-cmark and only touches HTML emission. If you're regenerating snapshots after a Stage 1 / `markdown_refs` / `markdown_links` change, expect raw wikilinks here, not the pre-Phase-3 `![](logo.png)` / `[text](moss-resolved:…)` shapes.
